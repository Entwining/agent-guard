mod argv;
mod arithmetic;
mod arrays;
mod brush;
mod cwd;
mod divergence;
pub mod lexer;
mod pipeline;
mod statements;
mod words;

use crate::{CheckError, CheckErrorKind, CoverageGap, limits::MAX_NESTING};
use std::ops::Range;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    StructuredOnly,
    Brush,
}

/// Parse comparators never contribute semantic observations.
pub const ACCEPTANCE_ARMS: &[Arm] = &[Arm::Brush];

#[derive(Debug, Clone)]
enum WordSyntax {
    ProcessInput(Box<ProcessInput>),
    Shell,
    Heredoc,
    Arithmetic,
    Literal,
}

#[derive(Debug, Clone)]
struct ProcessInput {
    body: Vec<Statement>,
    prefix: String,
}

#[derive(Debug, Clone)]
struct RawWord {
    raw: String,
    syntax: WordSyntax,
    expansions: Vec<RawExpansion>,
}
#[derive(Debug, Clone)]
enum RawExpansion {
    Code(String),
    Variable(String),
}
#[derive(Debug, Clone)]
struct RawRedirect {
    target: RawWord,
    direction: crate::record::Direction,
}
#[derive(Debug, Clone)]
enum Statement {
    Redirected(Vec<RawRedirect>, Vec<Statement>),
    UnsupportedSyntax,
    Group(Vec<Statement>),
    Subshell(Vec<Statement>),
    Substitution(Vec<Statement>),
    Async(Vec<Statement>),
    Definition(String, Vec<Statement>),
    Binary(Operator, Box<Statement>, Box<Statement>),
    Conditional {
        condition: Vec<Statement>,
        then: Vec<Statement>,
        otherwise: Vec<Statement>,
    },
    Loop {
        variable: Option<String>,
        header: Vec<RawWord>,
        body: Vec<Statement>,
        empty: bool,
    },
    Case {
        words: Vec<RawWord>,
        branches: Vec<Vec<Statement>>,
        exhaustive: bool,
    },
    Use(RawWord),
    Expansion(RawWord),
    ArrayAssignment {
        name: String,
        values: Vec<(Option<RawWord>, RawWord)>,
        append: bool,
        declaration: bool,
    },
    Command {
        assignments: Vec<(String, RawWord)>,
        argv: Vec<RawWord>,
        redirects: Vec<RawRedirect>,
        pipeline: Option<usize>,
    },
}

#[derive(Debug, Clone, Copy)]
enum Operator {
    And,
    Or,
    Pipe,
}

struct Parsed {
    records: Option<Vec<Statement>>,
    spans: Vec<Range<usize>>,
}

struct SourceSyntax {
    original: Option<Rc<Vec<Statement>>>,
    records: Option<Rc<Vec<Statement>>>,
    detection: Option<divergence::Detection>,
}

pub use crate::record::{Command as CommandRecord, Redirect, Word};

#[derive(Debug, Default)]
pub struct Observation {
    #[cfg(test)]
    pub array_words: usize,
    pub script: crate::record::Script,
    pub gaps: Vec<CoverageGap>,
    pub parse_successes: usize,
    pub parse_failures: usize,
    pub executable_qualifier: bool,
    pub word_coverage: Vec<WordCoverage>,
    pub array_tail_regions: Vec<ArrayTailRegions>,
    #[cfg(test)]
    candidate_pairs: usize,
    #[cfg(test)]
    pub(crate) source_entries: usize,
    #[cfg(test)]
    pub(crate) parse_builds: usize,
    #[cfg(test)]
    context_delta_entries: usize,
    #[cfg(test)]
    pub(crate) cwd_candidates: usize,
    #[cfg(test)]
    pub(crate) failure_copies: usize,
    #[cfg(test)]
    pub(crate) statement_visits: usize,
}

#[derive(Debug, Clone)]
pub struct ParameterRegion {
    pub range: Range<usize>,
    pub supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayTailRegions {
    pub source: String,
    pub ranges: Vec<Range<usize>>,
}

#[derive(Debug, Clone)]
pub struct WordCoverage {
    pub raw: String,
    pub parameters: Vec<ParameterRegion>,
    pub unsupported: bool,
}

impl Observation {
    fn gap(&mut self, gap: CoverageGap) {
        if !self.gaps.contains(&gap) {
            self.gaps.push(gap);
        }
    }
}

pub fn check_nesting(source: &str) -> Result<(), CheckError> {
    let mut depth: usize = 0;
    for byte in source.bytes() {
        match byte {
            b'(' | b'{' | b'[' => {
                depth += 1;
                if depth > MAX_NESTING {
                    return Err(CheckError {
                        kind: CheckErrorKind::ResourceLimit,
                    });
                }
            }
            b')' | b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

pub fn observe(
    source: &str,
    arm: Arm,
    home: &str,
    cwd: &str,
    zsh: bool,
) -> Result<Observation, CheckError> {
    observe_with_user(source, arm, home, cwd, None, zsh)
}

pub fn observe_with_user(
    source: &str,
    arm: Arm,
    home: &str,
    cwd: &str,
    user: Option<&str>,
    zsh: bool,
) -> Result<Observation, CheckError> {
    observe_with_deadline(source, arm, home, cwd, user, zsh, None)
}

pub(crate) fn observe_with_deadline(
    source: &str,
    arm: Arm,
    home: &str,
    cwd: &str,
    user: Option<&str>,
    zsh: bool,
    deadline: Option<std::time::Instant>,
) -> Result<Observation, CheckError> {
    let host = crate::record::HostFacts { home, user };
    let mut observation = Observation::default();
    let mut scope = statements::Scope::new(home, cwd);
    let mut evaluator = statements::Evaluator::new(Frontend { arm, zsh, host }, &mut observation);
    evaluator.deadline = deadline;
    evaluator.source(source, &mut scope, 0)?;
    evaluator.finish();
    Ok(observation)
}

#[derive(Clone, Copy)]
struct Frontend<'a> {
    arm: Arm,
    zsh: bool,
    host: crate::record::HostFacts<'a>,
}

impl statements::Evaluator<'_, '_> {
    fn parsed_source(&mut self, source: &str) -> Result<Rc<SourceSyntax>, CheckError> {
        crate::check_deadline(self.deadline)?;
        if let Some(parsed) = self.parse_cache.get(source) {
            return Ok(Rc::clone(parsed));
        }
        #[cfg(test)]
        {
            self.output.parse_builds += 1;
        }
        let original = brush::records(source, source)?;
        crate::check_deadline(self.deadline)?;
        let original_records = original.records.map(Rc::new);
        let lexical = match lexer::Lexed::scan(source) {
            Ok(lexical) => Some(lexical),
            Err(lexer::LexError::Nesting) => {
                return Err(CheckError {
                    kind: CheckErrorKind::ResourceLimit,
                });
            }
            Err(lexer::LexError::Unterminated { .. }) => None,
        };
        let detection = lexical
            .as_ref()
            .map(|lexical| divergence::detect_lexed(source, &original.spans, lexical))
            .transpose()?;
        let records = if let Some(detection) = &detection
            && detection.masked != source
        {
            brush::records(source, &detection.masked)?
                .records
                .map(Rc::new)
        } else {
            original_records.clone()
        };
        crate::check_deadline(self.deadline)?;
        let parsed = Rc::new(SourceSyntax {
            original: original_records,
            records,
            detection,
        });
        self.parse_cache
            .insert(source.to_owned(), Rc::clone(&parsed));
        Ok(parsed)
    }

    fn source(
        &mut self,
        source: &str,
        scope: &mut statements::Scope,
        depth: usize,
    ) -> Result<(), CheckError> {
        #[cfg(test)]
        {
            self.output.source_entries += 1;
        }
        let Frontend { arm, zsh, .. } = self.frontend;
        scope.zsh = zsh;
        // NUL is ignored even inside a word, rather than splitting its bytes.
        let without_nul = source.contains('\0').then(|| source.replace('\0', ""));
        let source = without_nul.as_deref().unwrap_or(source);
        crate::check_deadline(self.deadline)?;
        if depth > MAX_NESTING {
            return Err(CheckError {
                kind: CheckErrorKind::ResourceLimit,
            });
        }
        if arm == Arm::StructuredOnly {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
            return Ok(());
        }
        let parsed = self.parsed_source(source)?;
        crate::check_deadline(self.deadline)?;
        let source_id = self.output.parse_successes + self.output.parse_failures;
        if parsed.original.is_some() {
            self.output.parse_successes += 1;
        } else {
            self.output.script.parse_failed = true;
            self.output.parse_failures += 1;
        }
        let Some(detection) = &parsed.detection else {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
            return Ok(());
        };
        if !detection.array_tail_spans.is_empty() {
            self.output.array_tail_regions.push(ArrayTailRegions {
                source: source.to_owned(),
                ranges: detection.array_tail_spans.clone(),
            });
        }
        self.output.executable_qualifier |= detection.executable_qualifier;
        if detection.divergent {
            self.output.gap(if zsh {
                CoverageGap::ExecutorDivergence
            } else {
                CoverageGap::UnsupportedDialectConstruct
            });
        }
        if let Some(records) = &parsed.records {
            self.run(records, scope, depth, source_id, depth > 0)?;
        } else {
            if !detection.divergent {
                self.output.gap(CoverageGap::UnsupportedShellSyntax);
            }
        }
        for code in &detection.code {
            self.isolated_source(code, scope, depth + 1)?;
        }
        Ok(())
    }
}

fn expand_process_input(
    input: &ProcessInput,
    scope: &mut statements::Scope,
    evaluator: &mut statements::Evaluator<'_, '_>,
    depth: usize,
) -> Result<Vec<Expanded>, CheckError> {
    let start = evaluator.output.script.commands.len();
    let mut inner = scope.isolated();
    evaluator.run(
        &input.body,
        &mut inner,
        depth + 1,
        evaluator.output.parse_successes,
        true,
    )?;
    let output = pipeline::process_output(&evaluator.output.script.commands[start..]);
    let mut word = Word::literal(format!("{}__observed_stream__", input.prefix));
    word.stream = Some(Box::new(output.map_or(
        crate::record::StreamOutput::Unknown,
        crate::record::StreamOutput::Known,
    )));
    Ok(vec![Expanded {
        unknown_splitting: false,
        split: vec![word.clone()],
        word,
        positional: false,
        nested: Vec::new(),
        arithmetic: Vec::new(),
        references: Vec::new(),
        tilde: false,
        parameters: Vec::new(),
        unsupported: false,
        lexical_ranges: Vec::new(),
    }])
}

fn expand_positional_argv(
    raw: &RawWord,
    scope: &statements::Scope,
    evaluator: &mut statements::Evaluator<'_, '_>,
) -> Option<Vec<Expanded>> {
    let list = words::positional_list(&raw.raw)?;
    let sequences = scope.positional_sequences()?;
    if sequences.len() > 1 {
        evaluator.output.gap(CoverageGap::UnresolvedTarget);
    }
    let mut results = Vec::new();
    for mut arguments in sequences {
        if arguments.iter().any(|word| word.field_count_unknown) {
            evaluator.output.gap(CoverageGap::UnresolvedTarget);
            if list.offset != "1" || list.length.is_some() {
                evaluator.output.gap(CoverageGap::UnsupportedShellSyntax);
            }
        }
        let integer = |text: &str| {
            text.trim().parse::<isize>().ok().or_else(|| {
                scope
                    .contexts()
                    .get(text.trim())
                    .and_then(|value| value.parse().ok())
            })
        };
        let offset = integer(&list.offset);
        let length = list.length.as_deref().map(integer);
        if offset.is_none() || length.is_some_and(|value| value.is_none_or(|n| n < 0)) {
            evaluator.output.gap(CoverageGap::UnsupportedShellSyntax);
            return None;
        }
        let offset = offset?;
        let start = if offset < 0 {
            arguments.len().saturating_sub(offset.unsigned_abs())
        } else {
            (offset as usize).saturating_sub(1)
        };
        arguments = arguments
            .into_iter()
            .skip(start)
            .take(length.flatten().map_or(usize::MAX, |n| n as usize))
            .collect();
        if list.concatenate && list.quoted {
            let text = arguments
                .iter()
                .map(Word::as_str)
                .collect::<Vec<_>>()
                .join(" ");
            let mut joined = Word::literal(text);
            for argument in &arguments {
                joined.expands |= argument.expands;
                joined.runtime_unknown |= argument.runtime_unknown;
                joined.shell_matches |= argument.shell_matches;
                joined.cardinality_unknown |= argument.cardinality_unknown;
                joined.expands |= argument.cardinality_unknown;
                for name in &argument.vars {
                    if !joined.vars.contains(name) {
                        joined.vars.push(name.clone());
                    }
                }
            }
            arguments = vec![joined];
        }
        let word = arguments
            .first()
            .cloned()
            .unwrap_or_else(|| Word::literal(String::new()));
        results.push(Expanded {
            unknown_splitting: false,
            word,
            split: arguments,
            positional: true,
            nested: Vec::new(),
            arithmetic: Vec::new(),
            references: Vec::new(),
            tilde: false,
            parameters: Vec::new(),
            unsupported: false,
            lexical_ranges: Vec::new(),
        });
    }
    Some(results)
}

fn expand_scoped(
    raw: &RawWord,
    scope: &mut statements::Scope,
    evaluator: &mut statements::Evaluator<'_, '_>,
    depth: usize,
    observe_bindings: bool,
) -> Result<Vec<Expanded>, CheckError> {
    if let WordSyntax::ProcessInput(input) = &raw.syntax {
        return expand_process_input(input, scope, evaluator, depth);
    }
    if let Some(arguments) = expand_positional_argv(raw, scope, evaluator) {
        return Ok(arguments);
    }
    if let Some(elements) = arrays::expand(raw, scope, evaluator, depth)? {
        return Ok(elements);
    }
    let host = evaluator.frontend.host;
    let first = scope.contexts();
    let cwd = scope.directory.current.render();
    let seed = words::expand_boxed(
        &raw.raw,
        &raw.syntax,
        &words::ExpansionContext {
            variables: &first,
            runtime_variables: &std::collections::BTreeSet::new(),
            pattern_variables: &scope.pattern_contexts(&first),
            host,
            cwd: &cwd,
            tilde_assigned: true,
        },
    )?;
    let mut contexts = vec![std::collections::BTreeMap::<String, Option<String>>::new()];
    let mut names = seed
        .word
        .vars
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    if !names.is_empty() && scope.bindings.contains_key("IFS") {
        names.insert("IFS".into());
    }
    for name in &names {
        if name.parse::<usize>().is_ok()
            && scope.bindings.get("#").is_some_and(|binding| {
                binding
                    .values
                    .iter()
                    .any(|value| matches!(value, statements::BindingValue::RuntimeDerived(_)))
            })
        {
            evaluator.output.gap(CoverageGap::UnsupportedShellSyntax);
        }
        if observe_bindings && seed.word.vars.contains(name) {
            evaluator.armed_reference(name, scope, depth)?;
        }
        if let Some(binding) = scope.bindings.get(name) {
            let mut next = Vec::new();
            for context in &contexts {
                for value in &binding.values {
                    // Preserve target inference from present lexical candidates;
                    // absence at a join is runtime data, not a scope refusal.
                    if matches!(value, statements::BindingValue::RuntimeUnknown(Some(value)) if value.is_empty())
                        && binding.values.iter().any(|value| {
                            !matches!(value, statements::BindingValue::RuntimeUnknown(Some(value)) if value.is_empty())
                        })
                    {
                        continue;
                    }
                    if let statements::BindingValue::RepeatedFields(repetition) = value {
                        for projection in repetition.projections() {
                            let mut context = context.clone();
                            context.insert(name.clone(), Some(projection));
                            if !next.contains(&context) {
                                if next.len() == 512 {
                                    evaluator.output.gap(CoverageGap::InspectionBudget);
                                    break;
                                }
                                next.push(context);
                            }
                        }
                        continue;
                    }
                    let mut context = context.clone();
                    if let Some(value) = value.lexical() {
                        context.insert(name.clone(), Some(value.clone()));
                    } else {
                        context.insert(name.clone(), None);
                        if value == &statements::BindingValue::Undetermined {
                            evaluator.output.gap(CoverageGap::UnsupportedShellSyntax);
                        }
                    }
                    if !next.contains(&context) {
                        if next.len() == 512 {
                            evaluator.output.gap(CoverageGap::InspectionBudget);
                            break;
                        }
                        next.push(context);
                    }
                }
            }
            contexts = next;
        }
    }
    let mut nested = Vec::new();
    for expansion in &raw.expansions {
        match expansion {
            RawExpansion::Code(code) => {
                if !nested.contains(code) {
                    nested.push(code.clone());
                }
            }
            RawExpansion::Variable(name) => {
                if let Some(binding) = scope.bindings.get(name).cloned() {
                    for value in binding.values {
                        if let statements::BindingValue::Known(code) = value {
                            if !nested.contains(&code) {
                                nested.push(code);
                            }
                        } else if value == statements::BindingValue::Undetermined {
                            evaluator.output.gap(CoverageGap::UnsupportedShellSyntax);
                        }
                    }
                }
            }
        }
    }
    let mut result = Vec::new();
    let mut context = first;
    for delta in contexts {
        #[cfg(test)]
        {
            evaluator.output.context_delta_entries += delta.len();
        }
        for (name, value) in delta {
            if let Some(value) = value {
                context.insert(name, value);
            } else {
                context.remove(&name);
            }
        }
        for tilde_assigned in if seed.tilde && context.contains_key("PWD") {
            vec![true, false]
        } else {
            vec![true]
        } {
            let mut runtime_variables = seed
                .word
                .vars
                .iter()
                .filter(|name| {
                    scope.bindings.get(*name).is_some_and(|binding| {
                        binding.values.iter().any(|value| {
                            matches!(value, statements::BindingValue::RuntimeUnknown(Some(value))
                                if context.get(*name) == Some(value))
                        })
                    })
                })
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            if scope
                .bindings
                .get("IFS")
                .is_some_and(|binding| binding.values.iter().any(|value| value.known().is_none()))
            {
                runtime_variables.insert("IFS".into());
            }
            let mut expanded = words::expand(
                &raw.raw,
                &raw.syntax,
                &words::ExpansionContext {
                    variables: &context,
                    runtime_variables: &runtime_variables,
                    pattern_variables: &scope.pattern_contexts(&context),
                    host,
                    cwd: &cwd,
                    tilde_assigned,
                },
            )?;
            let candidates = expanded
                .word
                .vars
                .iter()
                .filter_map(|name| context.get(name).map(|value| (name.clone(), value.clone())))
                .collect::<std::collections::BTreeMap<_, _>>();
            expanded.word.binding_candidates = candidates.clone();
            for word in &mut expanded.split {
                word.binding_candidates = candidates.clone();
            }
            let repeated = expanded
                .word
                .vars
                .iter()
                .filter(|name| {
                    scope.bindings.get(*name).is_some_and(|binding| {
                        binding.values.iter().any(|value| {
                            matches!(value, statements::BindingValue::RepeatedFields(_))
                        })
                    })
                })
                .collect::<Vec<_>>();
            if !repeated.is_empty() {
                if repeated
                    .iter()
                    .any(|name| words::parameter_affixes(&raw.raw, name).is_none())
                {
                    expanded.word.expands = true;
                    for word in &mut expanded.split {
                        word.expands = true;
                    }
                }
                // The literal fields are known; their repetition count is not.
                // Consumers whose roles depend on position need the unknown count.
                expanded.word.cardinality_unknown = true;
                expanded.word.field_count_unknown = repeated.iter().any(|name| {
                    words::parameter_affixes(&raw.raw, name).is_none_or(|(_, _, split)| split)
                });
                for word in &mut expanded.split {
                    word.cardinality_unknown = true;
                    word.field_count_unknown = expanded.word.field_count_unknown;
                }
            }
            if expanded.word.vars.iter().any(|name| {
                scope.bindings.get(name).is_some_and(|binding| {
                    binding.values.iter().any(|value| matches!(value,
                        statements::BindingValue::ShellMatches(text) | statements::BindingValue::ShellDerived(text) if context.get(name) == Some(&text.text)))
                })
            }) {
                expanded.word.shell_matches = true;
                for word in &mut expanded.split {
                    word.shell_matches = true;
                }
            }
            if expanded.word.vars.iter().any(|name| {
                scope.bindings.get(name).is_some_and(|binding| {
                    binding.values.iter().any(|value| {
                        matches!(
                            value,
                            statements::BindingValue::RuntimeDerived(_)
                                | statements::BindingValue::ShellDerived(_)
                        )
                    })
                })
            }) {
                expanded.word.expands = true;
                for word in &mut expanded.split {
                    word.expands = true;
                }
            }
            if expanded.unsupported
                && !evaluator.output.gaps.iter().any(|g| {
                    matches!(
                        g,
                        CoverageGap::ExecutorDivergence | CoverageGap::UnsupportedDialectConstruct
                    )
                })
            {
                evaluator.output.gap(CoverageGap::UnsupportedShellSyntax);
            }
            if expanded.unsupported || !expanded.parameters.is_empty() {
                evaluator.output.word_coverage.push(WordCoverage {
                    raw: raw.raw.clone(),
                    parameters: expanded.parameters.clone(),
                    unsupported: expanded.unsupported,
                });
            }
            for expression in &expanded.arithmetic {
                evaluator.armed_references(expression, scope, depth)?;
                for code in evaluator.arithmetic_code(expression, scope)? {
                    if !expanded.nested.contains(&code) {
                        expanded.nested.push(code);
                    }
                }
            }
            for expression in &expanded.references {
                evaluator.armed_references(expression, scope, depth)?;
            }
            for code in &expanded.nested {
                if !nested.contains(code) {
                    nested.push(code.clone());
                }
            }
            result.push(expanded);
        }
    }
    for code in nested {
        evaluator.isolated_source(&code, scope, depth + 1)?;
    }
    Ok(result)
}

struct Expanded {
    unknown_splitting: bool,
    positional: bool,
    split: Vec<Word>,
    word: Word,
    nested: Vec<String>,
    arithmetic: Vec<String>,
    references: Vec<String>,
    tilde: bool,
    parameters: Vec<ParameterRegion>,
    unsupported: bool,
    lexical_ranges: Vec<std::ops::Range<usize>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identical_sources_share_syntax_but_observe_each_context() {
        for width in [2, 4, 8, 16] {
            let mut output = Observation::default();
            let host = crate::record::HostFacts {
                home: "/synthetic/home",
                user: None,
            };
            let mut evaluator = statements::Evaluator::new(
                Frontend {
                    arm: Arm::Brush,
                    zsh: true,
                    host,
                },
                &mut output,
            );
            for index in 0..width {
                let mut scope =
                    statements::Scope::new(host.home, &format!("/synthetic/dir{index}"));
                scope.assign(
                    "file".into(),
                    vec![statements::BindingValue::Known(format!("public{index}"))],
                );
                evaluator
                    .source("cat \"$file\"", &mut scope, index % 2)
                    .unwrap();
            }
            assert_eq!(output.parse_builds, 1, "width={width}");
            assert_eq!(output.source_entries, width);
            assert_eq!(output.script.commands.len(), width);
            for (index, command) in output.script.commands.iter().enumerate() {
                assert_eq!(command.argv[1].text, format!("public{index}"));
                assert_eq!(command.cwd, format!("/synthetic/dir{index}"));
                assert_eq!(command.nested, index % 2 == 1);
            }
        }
    }

    #[test]
    fn alternative_environments_store_only_binding_deltas() {
        for width in [4, 16, 64, 256] {
            let mut output = Observation::default();
            let host = crate::record::HostFacts {
                home: "/synthetic/home",
                user: None,
            };
            let mut evaluator = statements::Evaluator::new(
                Frontend {
                    arm: Arm::Brush,
                    zsh: false,
                    host,
                },
                &mut output,
            );
            let mut scope = statements::Scope::new(host.home, "/synthetic/project");
            for index in 0..width {
                scope.assign(
                    format!("ambient{index}"),
                    vec![statements::BindingValue::Known("unchanged".into())],
                );
            }
            scope.assign(
                "file".into(),
                vec![
                    statements::BindingValue::Known("public".into()),
                    statements::BindingValue::RuntimeUnknown(Some("public".into())),
                    statements::BindingValue::Known("protected".into()),
                ],
            );
            let values = evaluator
                .expand(
                    &RawWord {
                        raw: "\"$file\"".into(),
                        syntax: WordSyntax::Shell,
                        expansions: Vec::new(),
                    },
                    &mut scope,
                    0,
                )
                .unwrap();
            assert_eq!(values.len(), 2);
            assert_eq!(values[0].word.text, "public");
            assert!(values[0].word.runtime_unknown);
            assert_eq!(values[1].word.text, "protected");
            assert!(!values[1].word.runtime_unknown);
            assert_eq!(output.context_delta_entries, 2, "ambient width={width}");
        }
    }

    #[test]
    fn syntax_cache_hits_keep_depth_deadline_and_failure_checks() {
        let mut output = Observation::default();
        let host = crate::record::HostFacts {
            home: "/synthetic/home",
            user: None,
        };
        let mut evaluator = statements::Evaluator::new(
            Frontend {
                arm: Arm::Brush,
                zsh: false,
                host,
            },
            &mut output,
        );
        let mut scope = statements::Scope::new(host.home, "/synthetic/project");
        evaluator.source("printf public", &mut scope, 0).unwrap();
        assert_eq!(
            evaluator
                .source("printf public", &mut scope, MAX_NESTING + 1)
                .unwrap_err()
                .kind,
            CheckErrorKind::ResourceLimit
        );
        evaluator.deadline = Some(std::time::Instant::now());
        assert_eq!(
            evaluator
                .source("printf public", &mut scope, 0)
                .unwrap_err()
                .kind,
            CheckErrorKind::Deadline
        );
        evaluator.deadline = None;
        for _ in 0..2 {
            evaluator.source("if", &mut scope, 0).unwrap();
        }
        assert_eq!(output.parse_builds, 2);
        assert_eq!(output.parse_failures, 2);
        assert!(output.script.parse_failed);
        assert!(output.gaps.contains(&CoverageGap::UnsupportedShellSyntax));
    }

    #[test]
    fn quoted_star_preserves_unknown_argument_metadata() {
        let observation = observe(
            "f() { cat \"$*\"; }; f $value public",
            Arm::Brush,
            "/synthetic/home",
            "/synthetic/project",
            true,
        )
        .unwrap();
        assert!(observation.script.commands.iter().any(|command| {
            command.argv.first().is_some_and(|word| word == "cat")
                && command.argv.get(1).is_some_and(|word| {
                    word.ends_with("public")
                        && word.runtime_unknown
                        && word.vars.contains(&"1".into())
                })
        }));
    }
    #[test]
    fn complete_alternative_argv() {
        assert!(
            !ACCEPTANCE_ARMS.is_empty(),
            "missing acceptance parser arms"
        );
        for &arm in ACCEPTANCE_ARMS {
            let obs = observe("p='public protected'; cat $p", arm, "/h", "/h/p", true).unwrap();
            assert!(
                obs.script
                    .commands
                    .iter()
                    .any(|c| c.argv == ["cat", "public", "protected"])
            );
            assert!(
                obs.script
                    .commands
                    .iter()
                    .any(|c| c.argv == ["cat", "public protected"])
            );
        }
    }
}
