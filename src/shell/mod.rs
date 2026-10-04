mod brush;
mod divergence;
pub mod lexer;
mod words;

use crate::{CheckError, CheckErrorKind, CoverageGap, limits::MAX_NESTING};
use std::{
    collections::{BTreeMap, VecDeque},
    ops::Range,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    StructuredOnly,
    Brush,
}

/// D25: parse comparators never contribute semantic observations.
pub const ACCEPTANCE_ARMS: &[Arm] = &[Arm::Brush];

#[derive(Debug, Clone)]
enum WordSyntax {
    Shell,
    Heredoc,
    Arithmetic,
    Literal,
}

#[derive(Debug, Clone)]
struct RawWord {
    raw: String,
    syntax: WordSyntax,
}
#[derive(Debug, Clone)]
struct RawRedirect {
    target: RawWord,
    direction: crate::record::Direction,
}
#[derive(Debug, Clone)]
enum Record {
    Nested(Vec<Record>),
    Definition(String, Vec<Record>),
    Assignment(String, RawWord),
    LoopBinding(String, Vec<RawWord>),
    Expansion(RawWord),
    Command {
        argv: Vec<RawWord>,
        redirects: Vec<RawRedirect>,
        pipeline: Option<usize>,
    },
}

struct Parsed {
    records: Option<Vec<Record>>,
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
}

#[derive(Debug, Clone)]
pub struct ParameterRegion {
    pub range: Range<usize>,
    pub supported: bool,
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
    let mut variables = BTreeMap::from([
        ("HOME".to_owned(), vec![home.to_owned()]),
        ("PWD".to_owned(), vec![cwd.to_owned()]),
    ]);
    let host = crate::record::HostFacts { home, user };
    let mut observation = Observation::default();
    observe_source(
        source,
        Frontend { arm, zsh, host },
        &mut variables,
        &mut observation,
        0,
    )?;
    Ok(observation)
}

#[derive(Clone, Copy)]
struct Frontend<'a> {
    arm: Arm,
    zsh: bool,
    host: crate::record::HostFacts<'a>,
}

fn observe_source(
    source: &str,
    frontend: Frontend<'_>,
    variables: &mut BTreeMap<String, Vec<String>>,
    output: &mut Observation,
    depth: usize,
) -> Result<(), CheckError> {
    let Frontend { arm, zsh, host } = frontend;
    if depth > MAX_NESTING {
        return Err(CheckError {
            kind: CheckErrorKind::ResourceLimit,
        });
    }
    if arm == Arm::StructuredOnly {
        output.gap(CoverageGap::UnsupportedShellSyntax);
        return Ok(());
    }
    let parse = |parsed: &str| match arm {
        Arm::Brush => brush::records(source, parsed),
        Arm::StructuredOnly => unreachable!(),
    };
    let original = parse(source)?;
    let source_id = output.parse_successes + output.parse_failures;
    if original.records.is_some() {
        output.parse_successes += 1;
    } else {
        output.script.parse_failed = true;
        output.parse_failures += 1;
    }
    let lexical = match lexer::Lexed::scan(source) {
        Ok(lexical) => lexical,
        Err(lexer::LexError::Nesting) => {
            return Err(CheckError {
                kind: CheckErrorKind::ResourceLimit,
            });
        }
        Err(lexer::LexError::Unterminated { .. }) => {
            output.gap(CoverageGap::UnsupportedShellSyntax);
            return Ok(());
        }
    };
    let detection = divergence::detect_lexed(source, &original.spans, &lexical)?;
    output.executable_qualifier |= detection.executable_qualifier;
    if detection.divergent {
        output.gap(if zsh {
            CoverageGap::ExecutorDivergence
        } else {
            CoverageGap::UnsupportedDialectConstruct
        });
    }
    let records = if detection.masked != source {
        parse(&detection.masked)?.records
    } else {
        original.records
    };
    let Some(records) = records else {
        if !detection.divergent {
            output.gap(CoverageGap::UnsupportedShellSyntax);
        }
        for code in &detection.code {
            observe_source(
                code,
                Frontend { arm, zsh, host },
                &mut variables.clone(),
                output,
                depth + 1,
            )?;
        }
        return Ok(());
    };
    let functions: BTreeMap<String, Vec<Record>> = records
        .iter()
        .filter_map(|record| {
            if let Record::Definition(name, body) = record {
                Some((name.clone(), body.clone()))
            } else {
                None
            }
        })
        .collect();
    let mut pending: VecDeque<_> = records.into_iter().map(|record| (record, false)).collect();
    let mut inspected = 0;
    while let Some((record, nested)) = pending.pop_front() {
        inspected += 1;
        if inspected > 512 {
            output.gap(CoverageGap::InspectionBudget);
            break;
        }
        match record {
            Record::Nested(records) => {
                for record in records.into_iter().rev() {
                    pending.push_front((record, true));
                }
            }
            Record::Definition(_, body) => {
                // Static preflight retains Go's conservative observation of declared bodies.
                for record in body.iter().rev() {
                    if !matches!(record,Record::Command {argv,..} if argv.first().is_some_and(|w|functions.contains_key(&w.raw)))
                    {
                        pending.push_front((record.clone(), nested));
                    }
                }
            }
            Record::Assignment(name, word) => {
                bind(name, &[word], variables, output, frontend, depth)?;
            }
            Record::LoopBinding(name, words) => {
                bind(name, &words, variables, output, frontend, depth)?
            }
            Record::Expansion(word) => {
                for expanded in expand_all(&word, variables, output, host)? {
                    for nested in &expanded.nested {
                        observe_source(
                            nested,
                            Frontend { arm, zsh, host },
                            &mut variables.clone(),
                            output,
                            depth + 1,
                        )?;
                    }
                }
            }
            Record::Command {
                argv,
                redirects,
                pipeline,
            } => {
                let mut alternatives = vec![Vec::new()];

                for word in &argv {
                    let mut choices = Vec::new();
                    for expanded in expand_all(word, variables, output, host)? {
                        for nested in &expanded.nested {
                            observe_source(
                                nested,
                                Frontend { arm, zsh, host },
                                &mut variables.clone(),
                                output,
                                depth + 1,
                            )?;
                        }
                        choices.push(expanded.split);
                        choices.push(vec![expanded.word]);
                    }
                    choices.dedup();
                    let mut next = Vec::new();
                    for argv in &alternatives {
                        for choice in &choices {
                            let mut argv = argv.clone();
                            argv.extend(choice.clone());
                            next.push(argv);
                        }
                    }
                    if next.len() > 512 {
                        output.gap(CoverageGap::InspectionBudget);
                        next.truncate(512);
                    }
                    alternatives = next;
                }
                if let Some(body) = alternatives
                    .first()
                    .and_then(|argv| argv.first())
                    .and_then(|name| functions.get(&name.text))
                {
                    for record in body.iter().rev() {
                        pending.push_front((record.clone(), nested));
                    }
                    continue;
                }
                let mut targets = Vec::new();
                for redirect in redirects {
                    for expanded in expand_all(&redirect.target, variables, output, host)? {
                        for nested in &expanded.nested {
                            observe_source(
                                nested,
                                Frontend { arm, zsh, host },
                                &mut variables.clone(),
                                output,
                                depth + 1,
                            )?;
                        }
                        targets.push(Redirect::from_word(expanded.word, redirect.direction));
                    }
                }
                // Assign target roles to each complete argv; never flatten operands before roles.
                let cwds = variables.get("PWD").cloned().unwrap_or_default();
                for argv in alternatives {
                    for cwd in &cwds {
                        output.script.commands.push(CommandRecord {
                            argv: argv.clone(),
                            redirects: targets.clone(),
                            pipeline: pipeline.map(|id| (source_id, id)),
                            cwd: cwd.clone(),
                            nested: nested || depth > 0,
                            program: (!argv.is_empty()).then_some(0),
                            wrappers: Vec::new(),
                            shell: true,
                            flags: Vec::new(),
                            items: None,
                            stdin: if targets.iter().any(|r| {
                                matches!(
                                    r.direction,
                                    crate::record::Direction::Heredoc
                                        | crate::record::Direction::Herestring
                                )
                            }) {
                                crate::record::Stdin::Data(
                                    targets
                                        .iter()
                                        .enumerate()
                                        .filter_map(|(i, r)| {
                                            matches!(
                                                r.direction,
                                                crate::record::Direction::Heredoc
                                                    | crate::record::Direction::Herestring
                                            )
                                            .then_some(i)
                                        })
                                        .collect(),
                                )
                            } else {
                                crate::record::Stdin::None
                            },
                        });
                        if argv.first().is_some_and(|s| s == "cd")
                            && let Some(target) = argv.iter().skip(1).find(|s| !s.starts_with('-'))
                        {
                            let next =
                                crate::filesystem::normalize(target, cwd, &variables["HOME"][0]);
                            let dirs = variables.entry("PWD".into()).or_default();
                            if !dirs.contains(&next) {
                                dirs.push(next);
                            }
                        }
                    }
                }
                if output.script.commands.len() > 512 {
                    output.gap(CoverageGap::InspectionBudget);
                    break;
                }
            }
        }
    }
    for name in &detection.evaluated_variables {
        if let Some(codes) = variables.get(name).cloned() {
            for code in codes {
                observe_source(
                    &code,
                    Frontend { arm, zsh, host },
                    &mut variables.clone(),
                    output,
                    depth + 1,
                )?;
            }
        }
    }
    for code in &detection.code {
        observe_source(
            code,
            Frontend { arm, zsh, host },
            &mut variables.clone(),
            output,
            depth + 1,
        )?;
    }
    Ok(())
}

fn bind(
    name: String,
    words: &[RawWord],
    variables: &mut BTreeMap<String, Vec<String>>,
    output: &mut Observation,
    frontend: Frontend<'_>,
    depth: usize,
) -> Result<(), CheckError> {
    let Frontend { arm, zsh, host } = frontend;
    let mut values = Vec::new();
    for word in words {
        for expanded in expand_all(word, variables, output, host)? {
            for code in &expanded.nested {
                observe_source(
                    code,
                    Frontend { arm, zsh, host },
                    &mut variables.clone(),
                    output,
                    depth + 1,
                )?;
            }
            if !values.contains(&expanded.word.text) {
                values.push(expanded.word.text);
            }
        }
    }
    if values.len() > 512 {
        output.gap(CoverageGap::InspectionBudget);
        values.truncate(512);
    }
    variables.insert(name, values);
    Ok(())
}

fn expand_all(
    raw: &RawWord,
    variables: &BTreeMap<String, Vec<String>>,
    output: &mut Observation,
    host: crate::record::HostFacts<'_>,
) -> Result<Vec<Expanded>, CheckError> {
    let mut contexts = vec![BTreeMap::new()];
    for (name, values) in variables {
        if !["HOME", "PWD"].contains(&name.as_str())
            && !raw.raw.contains(&format!("${name}"))
            && !raw.raw.contains(&format!("${{{name}"))
        {
            continue;
        }
        let mut next = Vec::new();
        'combinations: for context in &contexts {
            for value in values {
                if next.len() == 512 {
                    output.gap(CoverageGap::InspectionBudget);
                    break 'combinations;
                }
                let mut context = context.clone();
                context.insert(name.clone(), value.clone());
                next.push(context);
            }
        }
        contexts = next;
    }
    contexts
        .iter()
        .map(|context| {
            let expanded = words::expand(&raw.raw, &raw.syntax, context, host)?;
            if expanded.unsupported
                && !output.gaps.iter().any(|gap| {
                    matches!(
                        gap,
                        CoverageGap::ExecutorDivergence | CoverageGap::UnsupportedDialectConstruct
                    )
                })
            {
                output.gap(CoverageGap::UnsupportedShellSyntax);
            }
            if expanded.unsupported || !expanded.parameters.is_empty() {
                output.word_coverage.push(WordCoverage {
                    raw: raw.raw.clone(),
                    parameters: expanded.parameters.clone(),
                    unsupported: expanded.unsupported,
                });
            }
            Ok(expanded)
        })
        .collect()
}

struct Expanded {
    split: Vec<Word>,
    word: Word,
    nested: Vec<String>,
    parameters: Vec<ParameterRegion>,
    unsupported: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
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
