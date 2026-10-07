mod argv;
mod arithmetic;
mod brush;
mod cwd;
mod divergence;
pub mod lexer;
mod pipeline;
mod statements;
mod words;

use crate::{CheckError, CheckErrorKind, CoverageGap, limits::MAX_NESTING};
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    StructuredOnly,
    Brush,
}

/// D25: parse comparators never contribute semantic observations.
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
        values: Vec<RawWord>,
        append: bool,
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

pub use crate::record::{Command as CommandRecord, Redirect, Word};

#[derive(Debug, Default)]
pub struct Observation {
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
    pub(crate) cwd_candidates: usize,
    #[cfg(test)]
    pub(crate) failure_copies: usize,
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
        // mvdan's lexer ignores NUL bytes, including inside words (lexer.go:78-81).
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
        let original = brush::records(source, source)?;
        crate::check_deadline(self.deadline)?;
        let source_id = self.output.parse_successes + self.output.parse_failures;
        if original.records.is_some() {
            self.output.parse_successes += 1;
        } else {
            self.output.script.parse_failed = true;
            self.output.parse_failures += 1;
        }
        let lexical = match lexer::Lexed::scan(source) {
            Ok(lexical) => lexical,
            Err(lexer::LexError::Nesting) => {
                return Err(CheckError {
                    kind: CheckErrorKind::ResourceLimit,
                });
            }
            Err(lexer::LexError::Unterminated { .. }) => {
                self.output.gap(CoverageGap::UnsupportedShellSyntax);
                return Ok(());
            }
        };
        let detection = divergence::detect_lexed(source, &original.spans, &lexical)?;
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
        let records = if detection.masked != source {
            brush::records(source, &detection.masked)?.records
        } else {
            original.records
        };
        if let Some(records) = records {
            self.run(&records, scope, depth, source_id, depth > 0)?;
        } else {
            if !detection.divergent {
                self.output.gap(CoverageGap::UnsupportedShellSyntax);
            }
        }
        for code in detection.code {
            self.isolated_source(&code, scope, depth + 1)?;
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
    let mut arguments = scope.positional_words()?;
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
    } else if let Some(offset) = offset {
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
        return Some(vec![Expanded {
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
        }]);
    }
    None
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
    let host = evaluator.frontend.host;
    let first = scope.contexts();
    let cwd = scope.directory.current.render();
    let seed = words::expand(
        &raw.raw,
        &raw.syntax,
        &words::ExpansionContext {
            variables: &first,
            runtime_variables: &std::collections::BTreeSet::new(),
            host,
            cwd: &cwd,
            tilde_assigned: true,
        },
    )?;
    let mut contexts = vec![first];
    for name in seed
        .word
        .vars
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
    {
        if observe_bindings {
            evaluator.armed_reference(name, scope, depth)?;
        }
        if let Some(binding) = scope.bindings.get(name) {
            let mut next = Vec::new();
            for context in &contexts {
                for value in &binding.values {
                    // Preserve pre-M2 inference from present lexical candidates;
                    // absence at a join is runtime data, not a scope refusal.
                    if matches!(value, statements::BindingValue::RuntimeUnknown(Some(value)) if value.is_empty())
                        && binding.values.iter().any(|value| {
                            !matches!(value, statements::BindingValue::RuntimeUnknown(Some(value)) if value.is_empty())
                        })
                    {
                        continue;
                    }
                    if next.len() == 512 {
                        evaluator.output.gap(CoverageGap::InspectionBudget);
                        break;
                    }
                    let mut context = context.clone();
                    if let statements::BindingValue::Known(value)
                    | statements::BindingValue::RuntimeUnknown(Some(value))
                    | statements::BindingValue::RuntimeDerived(value)
                    | statements::BindingValue::ShellMatches(value)
                    | statements::BindingValue::ShellDerived(value) = value
                    {
                        context.insert(name.clone(), value.clone());
                    } else {
                        context.remove(name);
                        if value == &statements::BindingValue::Undetermined {
                            evaluator.output.gap(CoverageGap::UnsupportedShellSyntax);
                        }
                    }
                    next.push(context);
                }
            }
            contexts = next;
        }
    }
    for expansion in &raw.expansions {
        match expansion {
            RawExpansion::Code(code) => evaluator.isolated_source(code, scope, depth + 1)?,
            RawExpansion::Variable(name) => {
                if let Some(binding) = scope.bindings.get(name).cloned() {
                    for value in binding.values {
                        if let statements::BindingValue::Known(code) = value {
                            evaluator.isolated_source(&code, scope, depth + 1)?;
                        } else if value == statements::BindingValue::Undetermined {
                            evaluator.output.gap(CoverageGap::UnsupportedShellSyntax);
                        }
                    }
                }
            }
        }
    }
    let mut result = Vec::new();
    for context in contexts {
        for tilde_assigned in if seed.tilde && context.contains_key("PWD") {
            vec![true, false]
        } else {
            vec![true]
        } {
            let runtime_variables = seed
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
                .collect();
            let mut expanded = words::expand(
                &raw.raw,
                &raw.syntax,
                &words::ExpansionContext {
                    variables: &context,
                    runtime_variables: &runtime_variables,
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
            if expanded.word.vars.iter().any(|name| {
                scope.bindings.get(name).is_some_and(|binding| {
                    binding.values.iter().any(|value| matches!(value,
                        statements::BindingValue::ShellMatches(text) | statements::BindingValue::ShellDerived(text) if context.get(name) == Some(text)))
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
                evaluator.isolated_source(code, scope, depth + 1)?;
            }
            result.push(expanded);
        }
    }
    Ok(result)
}

struct Expanded {
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
