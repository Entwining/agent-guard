use super::cwd::{self, Directory};
use super::{Expanded, Frontend, Observation, Operator, RawWord, Statement, WordSyntax};
use crate::record::stream::{Builder as FlowBuilder, Flow, Guard, Origins, Output, compatible};
use crate::{
    CheckError, CoverageGap,
    limits::MAX_NESTING,
    record::{Command, Role, Stdin},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum BindingValue {
    Known(String),
    // Keep the uncommon repetition payload off the recursive evaluator stack.
    RepeatedFields(Box<LiteralRepetition>),
    Arguments(Vec<crate::record::Word>),
    Array(Box<super::arrays::IndexedArray>),
    // The lexical representative preserves target inference; it is
    // never evidence of the runtime value or its arithmetic contents.
    RuntimeUnknown(Option<String>),
    // Derived text must carry its runtime uncertainty through substitution.
    RuntimeDerived(String),
    ShellMatches(ShellValue),
    ShellDerived(ShellValue),
    Undetermined,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct ShellValue {
    pub text: String,
    pub quoted_ranges: Vec<std::ops::Range<usize>>,
}

impl ShellValue {
    pub fn from_word(word: &crate::record::Word) -> Self {
        Self {
            text: word.text.clone(),
            quoted_ranges: word.quoted_ranges.clone(),
        }
    }
}

impl From<String> for ShellValue {
    fn from(text: String) -> Self {
        Self {
            text,
            quoted_ranges: Vec::new(),
        }
    }
}

impl std::ops::Deref for ShellValue {
    type Target = str;
    fn deref(&self) -> &str {
        &self.text
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct LiteralRepetition {
    pub prefix: String,
    pub alternatives: Vec<String>,
    pub suffix: String,
    pub may_be_empty: bool,
}

impl LiteralRepetition {
    pub fn projections(&self) -> Vec<String> {
        let mut values = Vec::new();
        if self.may_be_empty {
            values.push(format!("{}{}", self.prefix, self.suffix));
        }
        for alternative in &self.alternatives {
            values.push(format!("{}{alternative}{}", self.prefix, self.suffix));
            if !self.suffix.is_empty() {
                // Only the final field receives a copied suffix; earlier fields
                // still reach the consumer with their original identities.
                values.push(format!("{}{alternative}", self.prefix));
            }
        }
        values
    }
}

impl BindingValue {
    pub(super) fn lexical(&self) -> Option<&String> {
        match self {
            Self::Known(value)
            | Self::RuntimeUnknown(Some(value))
            | Self::RuntimeDerived(value) => Some(value),
            Self::ShellMatches(value) | Self::ShellDerived(value) => Some(&value.text),
            Self::RepeatedFields(_)
            | Self::Arguments(_)
            | Self::Array(_)
            | Self::RuntimeUnknown(None)
            | Self::Undetermined => None,
        }
    }
    pub fn known(&self) -> Option<&String> {
        match self {
            Self::Known(value) => Some(value),
            Self::RepeatedFields(_)
            | Self::Arguments(_)
            | Self::Array(_)
            | Self::RuntimeUnknown(_)
            | Self::RuntimeDerived(_)
            | Self::ShellMatches(_)
            | Self::ShellDerived(_)
            | Self::Undetermined => None,
        }
    }
}

#[cfg(test)]
#[derive(Clone, Debug, Default)]
struct EntryCopies(Rc<std::cell::Cell<usize>>);

#[cfg(test)]
impl PartialEq for EntryCopies {
    fn eq(&self, _: &Self) -> bool {
        // Instrumentation does not change binding equality or loop convergence.
        true
    }
}
#[cfg(test)]
impl Eq for EntryCopies {}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Binding {
    origins: Option<Rc<Origins>>,
    pub values: Rc<Vec<BindingValue>>,
    // A tied Zsh update replaces its view without replacing Bash's independent value.
    pub(super) bash_values: Option<std::rc::Rc<Vec<BindingValue>>>,
    exported: bool,
    arithmetic: bool,
    #[cfg(test)]
    copies: EntryCopies,
}

impl Binding {
    fn join_is_identity(&self) -> bool {
        // Multi-value joins still deduplicate and normalize repetition/empty
        // candidates, even when every branch holds the same storage.
        self.values.len() <= 1
            && self
                .bash_values
                .as_ref()
                .is_none_or(|values| values.len() <= 1)
    }
}

impl Clone for Binding {
    fn clone(&self) -> Self {
        #[cfg(test)]
        self.copies.0.set(self.copies.0.get() + 1);
        Self {
            values: self.values.clone(),
            origins: self.origins.clone(),
            bash_values: self.bash_values.clone(),
            exported: self.exported,
            arithmetic: self.arithmetic,
            #[cfg(test)]
            copies: self.copies.clone(),
        }
    }
}

type LocalFrame = Rc<BTreeMap<String, Option<Binding>>>;

#[derive(Clone)]
pub(super) struct Scope {
    pub(super) named_dirs: std::rc::Rc<BTreeMap<String, Vec<BindingValue>>>,
    pub directory: Directory,
    pub bindings: Rc<BTreeMap<String, Binding>>,
    frames: Rc<Vec<LocalFrame>>,
    isolated: bool,
    defining: bool,
    conditional_definition: bool,
    returns: Rc<Vec<BindingState>>,
    loops: Rc<Vec<Rc<Vec<BindingState>>>>,
    summarizing_loop: bool,
    bounded_loop: bool,
    conditional_append: bool,
    piped: bool,
    pub(super) zsh: bool,
    pub(super) pipeline_input: Option<Flow>,
    captured: bool,
    output_fds: Rc<BTreeMap<i32, Option<usize>>>,
    input_fds: Rc<BTreeMap<i32, usize>>,
    stdin_id: usize,
    input_cursors: Rc<BTreeMap<usize, Flow>>,
    flow_guard: Rc<Guard>,
    flow_end: bool,
    relative_glob_moves: usize,
}

#[derive(Clone, PartialEq, Eq)]
struct BindingState {
    continue_loop: bool,
    flow_guard: Rc<Guard>,
    named_dirs: std::rc::Rc<BTreeMap<String, Vec<BindingValue>>>,
    bindings: Rc<BTreeMap<String, Binding>>,
    frames: Rc<Vec<LocalFrame>>,
}

fn cdpath_alias(name: &str) -> Option<&'static str> {
    match name {
        "CDPATH" => Some("cdpath"),
        "cdpath" => Some("CDPATH"),
        _ => None,
    }
}

fn restore_prefix(
    bindings: &mut Rc<BTreeMap<String, Binding>>,
    prior: &BTreeMap<String, Option<Binding>>,
) {
    for (name, binding) in prior {
        if let Some(binding) = binding {
            Rc::make_mut(bindings).insert(name.clone(), binding.clone());
        } else {
            Rc::make_mut(bindings).remove(name);
        }
    }
}

#[derive(Clone)]
struct Function {
    body: Rc<Vec<Statement>>,
    source_id: usize,
    exported: bool,
}

pub(super) struct Evaluator<'a, 'b> {
    pub frontend: Frontend<'a>,
    pub output: &'b mut Observation,
    functions: Rc<BTreeMap<String, Function>>,
    running: BTreeSet<String>,
    inspected: usize,
    function_runs: usize,
    unresolved_calls: Vec<(usize, bool, Option<usize>)>,
    function_namespaces: Vec<Rc<BTreeMap<String, Function>>>,
    namespace: Option<usize>,
    pub(super) deadline: Option<std::time::Instant>,
    pub(super) parse_cache: BTreeMap<String, std::rc::Rc<super::SourceSyntax>>,
    pub(super) flow: FlowBuilder,
}

pub(super) fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || i > 0 && b.is_ascii_digit())
}

#[derive(Clone)]
struct ArgumentAlternative {
    words: Rc<Vec<crate::record::Word>>,
    bindings: Rc<BTreeMap<String, BTreeSet<String>>>,
}

impl ArgumentAlternative {
    fn new(words: Vec<crate::record::Word>) -> Self {
        let mut alternative = Self {
            words: Rc::new(words),
            bindings: Rc::default(),
        };
        let words = alternative.words.clone();
        alternative.include_bindings(&words);
        alternative
    }

    fn include_bindings(&mut self, words: &[crate::record::Word]) {
        // A multi-field expansion can carry different values for the same
        // binding. Later choices must agree with every preceding value.
        for word in words
            .iter()
            .filter(|word| !matches!(word.role, Role::Assign | Role::Precommand))
        {
            for (name, value) in &word.binding_candidates {
                if !self
                    .bindings
                    .get(name)
                    .is_some_and(|values| values.contains(value))
                {
                    Rc::make_mut(&mut self.bindings)
                        .entry(name.clone())
                        .or_default()
                        .insert(value.clone());
                }
            }
        }
    }

    fn compatible(&self, choice: &[crate::record::Word], #[cfg(test)] checks: &mut usize) -> bool {
        choice.iter().all(|word| {
            #[cfg(test)]
            {
                *checks += 1;
            }
            word.binding_candidates.iter().all(|(name, value)| {
                self.bindings
                    .get(name)
                    .is_none_or(|values| values.iter().all(|previous| previous == value))
            })
        })
    }

    fn extend(&mut self, choice: &[crate::record::Word], #[cfg(test)] copies: &mut usize) {
        #[cfg(test)]
        {
            *copies += choice.len();
            if Rc::strong_count(&self.words) > 1 {
                *copies += self.words.len();
            }
        }
        self.include_bindings(choice);
        Rc::make_mut(&mut self.words).extend(choice.iter().cloned());
    }

    fn into_words(self) -> Vec<crate::record::Word> {
        Rc::try_unwrap(self.words).unwrap_or_else(|shared| shared.as_ref().clone())
    }
}

pub(super) fn assignment(raw: &str) -> Option<(&str, &str)> {
    raw.split_once('=').filter(|(n, _)| identifier(n))
}

mod accumulation;
mod bindings_builtin;
mod child;
mod command;
mod compound;
mod declaration;
mod directory;
mod emit;
mod expansion;
mod lifecycle;
mod logical;
mod loop_analysis;
mod merge;
mod output;
mod pipeline;
mod positionals;
mod read;
mod repetition;
mod scope;
mod scope_assignments;
mod scope_candidates;
mod scope_flow;
mod scope_join;
use repetition::widen_runtime_repetition;

#[cfg(test)]
mod tests;

mod loops;

mod loop_header;
use loop_header::LoopHeader;

mod command_arguments;
mod command_assignments;
mod command_builtin;
mod command_replay;
