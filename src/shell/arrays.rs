use super::{
    Expanded, RawWord, WordSyntax,
    statements::{BindingValue, Evaluator, Scope},
    words,
};
use crate::{CheckError, record::Word};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct IndexedArray {
    pub base: i64,
    pub elements: BTreeMap<i64, Vec<Word>>,
    pub unknown: Vec<Word>,
    // Normalized start index and resource candidates of an append repetition.
    pub tail: Option<(i64, Vec<Word>)>,
    pub exact: bool,
    // Keep the two executor readings distinct; binding joins merge within each.
    pub zsh: Option<Box<IndexedArray>>,
}

impl IndexedArray {
    pub fn scalar(zsh: bool, values: &[BindingValue]) -> Self {
        let mut state = Self::new(zsh);
        state.push(binding_words(values));
        state
    }
    pub fn new(zsh: bool) -> Self {
        let mut state = Self::single(0);
        if zsh {
            state.zsh = Some(Box::new(Self::single(1)));
        }
        state
    }
    fn unknown(zsh: bool) -> Self {
        let mut state = Self::new(zsh);
        let mut word = Word::literal(String::new());
        word.expands = true;
        word.runtime_unknown = true;
        state.unknown.push(word.clone());
        state.exact = false;
        if let Some(state) = &mut state.zsh {
            state.unknown.push(word);
            state.exact = false;
        }
        state
    }
    fn single(base: i64) -> Self {
        Self {
            base,
            elements: BTreeMap::new(),
            unknown: Vec::new(),
            tail: None,
            exact: true,
            zsh: None,
        }
    }
    pub fn merge(&mut self, other: &Self) {
        if let Some(other) = &other.zsh {
            if let Some(zsh) = &mut self.zsh {
                zsh.merge(other);
            } else {
                self.zsh = Some(other.clone());
            }
        }
        self.exact &= other.exact && self.elements.keys().eq(other.elements.keys());
        if let Some((start, words)) = &other.tail {
            for (index, values) in &mut self.elements {
                if index >= start && !other.elements.contains_key(index) {
                    union(values, words);
                }
            }
        }
        for (index, words) in &other.elements {
            if !self.elements.contains_key(index)
                && let Some((start, tail)) = &self.tail
                && index >= start
            {
                union(self.elements.entry(*index).or_default(), tail);
            }
            union(self.elements.entry(*index).or_default(), words);
        }
        union(&mut self.unknown, &other.unknown);
        if let Some((start, words)) = &other.tail {
            self.extend_tail(*start, words);
        }
    }
    fn extend_tail(&mut self, start: i64, words: &[Word]) {
        let tail = self.tail.get_or_insert_with(|| (start, Vec::new()));
        tail.0 = tail.0.min(start);
        union(&mut tail.1, words);
    }
    pub fn widen_append(&mut self, prior: &Self) -> bool {
        let mut changed = false;
        if let Some(zsh) = &mut self.zsh
            && let Some(before) = &prior.zsh
        {
            changed |= zsh.widen_append(before);
        }
        if self.elements.len() <= prior.elements.len()
            || !prior.elements.iter().all(|(index, words)| {
                self.elements
                    .get(index)
                    .is_some_and(|current| words.iter().all(|word| current.contains(word)))
            })
        {
            return changed;
        }
        // The append backedge can repeat these identities at any later index.
        // Keep the stable prefix ordered and the tail as an unknown-width union.
        let appended = self
            .elements
            .keys()
            .filter(|index| !prior.elements.contains_key(index))
            .copied()
            .collect::<Vec<_>>();
        for index in appended {
            if let Some(words) = self.elements.remove(&index) {
                self.extend_tail(index, &words);
            }
        }
        self.exact = false;
        true
    }
    pub fn repeated(&self) -> bool {
        self.tail.is_some() || self.zsh.as_deref().is_some_and(Self::repeated)
    }
    fn push(&mut self, words: Vec<Word>) {
        self.exact &= words.iter().all(|word| !word.field_count_unknown);
        if let Some(zsh) = &mut self.zsh {
            zsh.push(words.clone());
        }
        self.push_one(words);
    }
    fn push_one(&mut self, words: Vec<Word>) {
        let next = self
            .elements
            .keys()
            .next_back()
            .map_or(0, |index| index + 1);
        self.elements.insert(next, words);
    }
    fn set_index(&mut self, index: Option<i64>, words: Vec<Word>) {
        if index == Some(0) {
            self.zsh = None;
        } else if let Some(zsh) = &mut self.zsh {
            zsh.set_index(index, words.clone());
        }
        self.set_one_index(index, words);
    }
    fn set_one_index(&mut self, index: Option<i64>, words: Vec<Word>) {
        if let Some(index) = index {
            let index = if index < 0 {
                self.elements.keys().next_back().map_or(0, |last| last + 1) + index
            } else {
                index - self.base
            };
            self.elements.insert(index, words);
        } else {
            union(&mut self.unknown, &words);
            self.exact = false;
        }
    }
    fn append_view(&mut self, other: &Self, index: Option<&RawWord>, scope: &Scope) {
        self.exact &= other.exact;
        for (offset, words) in other.elements.values().enumerate() {
            if let Some(index) = index {
                self.set_one_index(
                    integer(&index.raw, scope).map(|index| index + offset as i64),
                    words.clone(),
                );
            } else if self.exact {
                self.push_one(words.clone());
            } else if let Some((_, tail)) = &mut self.tail {
                union(tail, words);
            } else {
                union(&mut self.unknown, words);
            }
        }
        union(&mut self.unknown, &other.unknown);
        if let Some((start, words)) = &other.tail {
            self.extend_tail(*start, words);
        }
    }
    pub fn binding_values(&self) -> Vec<BindingValue> {
        let mut values = Vec::new();
        for state in self.zsh.as_deref().into_iter().chain(std::iter::once(self)) {
            for word in state
                .elements
                .values()
                .flatten()
                .chain(&state.unknown)
                .chain(state.tail.as_ref().into_iter().flat_map(|(_, words)| words))
            {
                let value = if word.runtime_unknown {
                    BindingValue::RuntimeUnknown(Some(word.text.clone()))
                } else if word.expands {
                    BindingValue::RuntimeDerived(word.text.clone())
                } else if word.globs || word.shell_matches {
                    BindingValue::ShellMatches(super::statements::ShellValue::from_word(word))
                } else {
                    BindingValue::Known(word.text.clone())
                };
                if !values.contains(&value) {
                    values.push(value);
                }
            }
        }
        values
    }
}

fn union(destination: &mut Vec<Word>, words: &[Word]) {
    for word in words {
        if !destination.contains(word) {
            destination.push(word.clone());
        }
    }
}

pub(super) fn array(scope: &Scope, name: &str) -> Option<IndexedArray> {
    let mut result: Option<IndexedArray> = None;
    for value in scope.bindings.get(name)?.values.iter() {
        if let BindingValue::Array(value) = value {
            if let Some(result) = &mut result {
                result.merge(value);
            } else {
                result = Some((**value).clone());
            }
        }
    }
    if let Some(result) = &mut result {
        let other = scope
            .bindings
            .get(name)?
            .values
            .iter()
            .filter(|value| !matches!(value, BindingValue::Array(_)))
            .cloned()
            .collect::<Vec<_>>();
        if !other.is_empty() {
            let words = binding_words(&other);
            union(result.elements.entry(0).or_default(), &words);
            if let Some(zsh) = &mut result.zsh {
                union(zsh.elements.entry(0).or_default(), &words);
                zsh.exact = false;
            }
            result.exact = false;
        }
    }
    result
}

pub(super) fn scalar_contexts(scope: &Scope) -> BTreeMap<String, String> {
    let mut contexts = BTreeMap::new();
    for name in scope.bindings.keys().filter(|name| !name.contains('[')) {
        if let Some(state) = array(scope, name) {
            for (index, words) in &state.elements {
                if let Some(word) = words.first().filter(|word| !word.expands) {
                    contexts.insert(format!("{name}[{index}]"), word.text.clone());
                    if *index == 0 {
                        contexts.insert(name.clone(), word.text.clone());
                    }
                }
            }
        }
    }
    contexts
}

fn binding_words(values: &[BindingValue]) -> Vec<Word> {
    let mut words = Vec::new();
    for value in values {
        let texts = match value {
            BindingValue::RepeatedFields(value) => value.projections(),
            BindingValue::Known(text)
            | BindingValue::RuntimeDerived(text)
            | BindingValue::RuntimeUnknown(Some(text)) => vec![text.clone()],
            BindingValue::ShellMatches(value) | BindingValue::ShellDerived(value) => {
                vec![value.text.clone()]
            }
            BindingValue::Array(_) | BindingValue::Arguments(_) => continue,
            BindingValue::RuntimeUnknown(None) | BindingValue::Undetermined => vec![String::new()],
        };
        for text in texts {
            let mut word = Word::literal(text);
            if let BindingValue::ShellMatches(value) | BindingValue::ShellDerived(value) = value {
                word.quoted_ranges = value.quoted_ranges.clone();
            }
            word.expands = matches!(
                value,
                BindingValue::RuntimeDerived(_)
                    | BindingValue::RuntimeUnknown(None)
                    | BindingValue::Undetermined
            );
            word.runtime_unknown = matches!(
                value,
                BindingValue::RuntimeUnknown(_) | BindingValue::RuntimeDerived(_)
            );
            word.shell_matches = matches!(
                value,
                BindingValue::ShellMatches(_) | BindingValue::ShellDerived(_)
            );
            word.cardinality_unknown = matches!(value, BindingValue::RepeatedFields(_));
            word.field_count_unknown = word.cardinality_unknown;
            union(&mut words, &[snapshot(word)]);
        }
    }
    words
}

fn integer(text: &str, scope: &Scope) -> Option<i64> {
    let text = text.trim();
    let name = text.strip_prefix('$').unwrap_or(text);
    let name = name
        .strip_prefix('{')
        .and_then(|name| name.strip_suffix('}'))
        .unwrap_or(name);
    text.parse().ok().or_else(|| {
        let values = &scope.bindings.get(name)?.values;
        let [BindingValue::Known(value)] = values.as_slice() else {
            return None;
        };
        value.parse().ok()
    })
}

fn indices(text: &str, scope: &Scope) -> Option<Vec<i64>> {
    if let Some(index) = integer(text, scope) {
        return Some(vec![index]);
    }
    let name = text.trim().strip_prefix('$').unwrap_or(text.trim());
    scope
        .bindings
        .get(name)?
        .values
        .iter()
        .map(|value| value.known()?.parse().ok())
        .collect()
}

pub(super) fn bash_only(mut values: Vec<BindingValue>) -> Vec<BindingValue> {
    for value in &mut values {
        if let BindingValue::Array(array) = value {
            array.zsh = None;
        }
    }
    values
}

fn snapshot(mut word: Word) -> Word {
    word.vars.clear();
    word.binding_candidates.clear();
    // Assignment has consumed lexical tilde expansion. Reading the stored
    // value cannot turn a runtime tilde back into HOME.
    word.raw = format!("'{}'", word.text.replace('\'', "'\\''"));
    word
}

mod bindings;
mod evaluator;
mod expansion;
pub(super) use bindings::{store, tied_cdpath, update};
pub(super) use expansion::expand;
#[cfg(test)]
mod tests;
