mod arithmetic;
mod brush;
mod divergence;
pub mod lexer;
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
    Shell,
    Heredoc,
    Arithmetic,
    Literal,
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
    let host = crate::record::HostFacts { home, user };
    let mut observation = Observation::default();
    let mut scope = statements::Scope::new(home, cwd);
    let mut evaluator = statements::Evaluator::new(Frontend { arm, zsh, host }, &mut observation);
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
        let Frontend { arm, zsh, .. } = self.frontend;
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
            self.source(&code, &mut scope.isolated(), depth + 1)?;
        }
        Ok(())
    }
}

fn expand_scoped(
    raw: &RawWord,
    scope: &mut statements::Scope,
    evaluator: &mut statements::Evaluator<'_, '_>,
    depth: usize,
) -> Result<Vec<Expanded>, CheckError> {
    let host = evaluator.frontend.host;
    let first = scope.contexts();
    let seed = words::expand(&raw.raw, &raw.syntax, &first, host)?;
    let mut contexts = vec![first];
    for name in seed
        .word
        .vars
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
    {
        if let Some(binding) = scope.bindings.get(name) {
            let mut next = Vec::new();
            for context in &contexts {
                for value in &binding.values {
                    if next.len() == 512 {
                        evaluator.output.gap(CoverageGap::InspectionBudget);
                        break;
                    }
                    let mut context = context.clone();
                    if let Some(value) = value {
                        context.insert(name.clone(), value.clone());
                    } else {
                        context.remove(name);
                        evaluator.output.gap(CoverageGap::UnsupportedShellSyntax);
                    }
                    next.push(context);
                }
            }
            contexts = next;
        }
    }
    for expansion in &raw.expansions {
        match expansion {
            RawExpansion::Code(code) => evaluator.source(code, &mut scope.isolated(), depth + 1)?,
            RawExpansion::Variable(name) => {
                if let Some(binding) = scope.bindings.get(name).cloned() {
                    for value in binding.values {
                        if let Some(code) = value {
                            evaluator.source(&code, &mut scope.isolated(), depth + 1)?;
                        } else {
                            evaluator.output.gap(CoverageGap::UnsupportedShellSyntax);
                        }
                    }
                }
            }
        }
    }
    let mut result = Vec::new();
    for context in contexts {
        let mut expanded = words::expand(&raw.raw, &raw.syntax, &context, host)?;
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
            for code in evaluator.arithmetic_code(expression, scope)? {
                if !expanded.nested.contains(&code) {
                    expanded.nested.push(code);
                }
            }
        }
        for code in &expanded.nested {
            evaluator.source(code, &mut scope.isolated(), depth + 1)?;
        }
        result.push(expanded);
    }
    Ok(result)
}

struct Expanded {
    split: Vec<Word>,
    word: Word,
    nested: Vec<String>,
    arithmetic: Vec<String>,
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
