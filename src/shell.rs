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
    fd: i32,
    duplicate: bool,
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
    Pipeline(Vec<Statement>),
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
    logical_guard_visits: usize,
    #[cfg(test)]
    pub(crate) flow_nodes: usize,
    #[cfg(test)]
    pub(crate) flow_pairs: usize,
    #[cfg(test)]
    pub(crate) flow_visits: usize,
    #[cfg(test)]
    pub(crate) flow_guard_pairs: usize,
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
    argv_prefix_word_copies: usize,
    #[cfg(test)]
    argument_compatibility_checks: usize,
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
    #[cfg(test)]
    pub(crate) header_comparisons: usize,
    #[cfg(test)]
    pub(crate) word_candidates_max: usize,
    #[cfg(test)]
    pub(crate) expansion_size: usize,
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
    let (lexical, error) = lexer::Lexed::parameter_fragment(source, lexer::Context::default());
    if error == Some(lexer::LexError::Nesting) {
        return Err(CheckError {
            kind: CheckErrorKind::ResourceLimit,
        });
    }
    let mut depth: usize = 0;
    for (at, byte) in source.bytes().enumerate() {
        let context = lexical.context(at);
        if !context.unquoted() || context.heredoc.is_some() || context.parameter_depth > 0 {
            continue;
        }
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

struct Expanded {
    named_tildes: std::collections::BTreeSet<String>,
    modifiers: bool,
    unset_parameters: std::collections::BTreeSet<String>,
    empty_parameters: std::collections::BTreeSet<String>,
    unknown_splitting: bool,
    positional: bool,
    split: Vec<Word>,
    word: Word,
    nested: Vec<String>,
    arithmetic: Vec<String>,
    references: Vec<String>,
    assignments: Vec<(String, String)>,
    tilde: bool,
    parameters: Vec<ParameterRegion>,
    unsupported: bool,
    lexical_ranges: Vec<std::ops::Range<usize>>,
}

mod candidates;
mod scoped;
mod source;
use candidates::expand_candidates;
use scoped::expand_scoped;
#[cfg(test)]
mod tests;
