use super::{
    Expanded, RawWord, WordSyntax,
    statements::{BindingValue, Evaluator, Scope},
    words,
};
use crate::{
    CheckError, CoverageGap,
    record::{Direction, Redirect, StreamOutput, Word},
};
use brush_parser::{
    ParserOptions,
    word::{self, Parameter, ParameterExpr, WordPiece, WordPieceWithSource},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
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
    for value in &scope.bindings.get(name)?.values {
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

pub(super) fn update(scope: &mut Scope, name: &str, values: &mut Vec<BindingValue>) {
    if values
        .iter()
        .any(|value| matches!(value, BindingValue::Array(_)))
    {
        return;
    }
    if let Some((base, index)) = name
        .split_once('[')
        .and_then(|(base, index)| Some((base, index.strip_suffix(']')?)))
    {
        let mut state = array(scope, base).unwrap_or_else(|| {
            scope.bindings.get(base).map_or_else(
                || IndexedArray::new(scope.zsh),
                |binding| IndexedArray::scalar(scope.zsh, &binding.values),
            )
        });
        let words = binding_words(values);
        state.set_index(integer(index, scope), words);
        scope.assign(base.into(), vec![BindingValue::Array(Box::new(state))]);
    } else if let Some(mut state) = array(scope, name) {
        let words = binding_words(values);
        state.elements.insert(0, words.clone());
        if let Some(zsh) = &mut state.zsh {
            zsh.elements.insert(0, words);
        }
        scope.assign_binding(format!("{name}[0]"), values.clone());
        *values = vec![BindingValue::Array(Box::new(state))];
    }
}

pub(super) fn store(scope: &mut Scope, name: &str, state: IndexedArray) {
    let prefix = format!("{name}[");
    scope
        .bindings
        .retain(|key, _| key != name && !key.starts_with(&prefix));
    for (index, words) in &state.elements {
        let values = words
            .iter()
            .map(|word| {
                if word.runtime_unknown || word.expands {
                    BindingValue::RuntimeUnknown(Some(word.text.clone()))
                } else if word.globs || word.shell_matches {
                    BindingValue::ShellMatches(super::statements::ShellValue::from_word(word))
                } else {
                    BindingValue::Known(word.text.clone())
                }
            })
            .collect();
        scope.assign_binding(format!("{name}[{}]", index + state.base), values);
    }
    scope.assign(name.into(), vec![BindingValue::Array(Box::new(state))]);
}

fn snapshot(mut word: Word) -> Word {
    word.vars.clear();
    word.binding_candidates.clear();
    // Assignment has consumed lexical tilde expansion. Reading the stored
    // value cannot turn a runtime tilde back into HOME.
    word.raw = format!("'{}'", word.text.replace('\'', "'\\''"));
    word
}

impl Evaluator<'_, '_> {
    pub(super) fn array_assignment(
        &mut self,
        name: &str,
        values: &[(Option<RawWord>, RawWord)],
        append: bool,
        declaration: bool,
        scope: &mut Scope,
        depth: usize,
    ) -> Result<(), CheckError> {
        let mut state = if append {
            array(scope, name).unwrap_or_else(|| IndexedArray::new(self.frontend.zsh))
        } else {
            IndexedArray::new(self.frontend.zsh)
        };
        for (index, raw) in values {
            let mut addition: Option<IndexedArray> = None;
            for expanded in self.expand(raw, scope, depth)? {
                let mut candidate = IndexedArray::new(self.frontend.zsh);
                candidate.exact &= !expanded.unknown_splitting;
                let zsh_unsplit = self.frontend.zsh
                    && !expanded.positional
                    && expanded.split.len() > 1
                    && !expanded.word.expands
                    && !expanded.word.runtime_unknown;
                for word in &expanded.split {
                    #[cfg(test)]
                    {
                        self.output.array_words += 1;
                    }
                    candidate.exact &= !word.field_count_unknown;
                    candidate.push(vec![snapshot(word.clone())]);
                }
                if let Some(zsh) = &mut candidate.zsh {
                    zsh.exact &= !expanded.unknown_splitting;
                    if zsh_unsplit {
                        zsh.elements.clear();
                        zsh.push_one(vec![snapshot(expanded.word)]);
                    }
                }
                if let Some(addition) = &mut addition {
                    addition.merge(&candidate);
                } else {
                    addition = Some(candidate);
                }
            }
            if let Some(addition) = addition {
                if index
                    .as_ref()
                    .is_some_and(|index| integer(&index.raw, scope) == Some(0))
                {
                    state.zsh = None;
                }
                if let Some(zsh) = &mut state.zsh
                    && let Some(other) = &addition.zsh
                {
                    zsh.append_view(other, index.as_ref(), scope);
                }
                state.append_view(&addition, index.as_ref(), scope);
            }
        }
        if declaration && scope.in_function() {
            scope.local(name);
        }
        store(scope, name, state);
        Ok(())
    }

    pub(super) fn read_array_fields(
        &mut self,
        name: &str,
        input: Option<&[String]>,
        scope: &mut Scope,
    ) {
        let mut state: Option<IndexedArray> = None;
        let contexts = scope.ifs_candidates();
        if scope
            .bindings
            .get("IFS")
            .is_some_and(|binding| binding.values.iter().any(|value| value.known().is_none()))
        {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
        }
        for value in input.into_iter().flatten() {
            for ifs in &contexts {
                let mut candidate = IndexedArray::new(self.frontend.zsh);
                for field in words::fields(value, &[], ifs.as_deref()) {
                    candidate.push(vec![Word::literal(field.into())]);
                }
                if let Some(state) = &mut state {
                    state.merge(&candidate);
                } else {
                    state = Some(candidate);
                }
            }
        }
        let mut state = state.unwrap_or_else(|| {
            let mut state = IndexedArray::new(self.frontend.zsh);
            let mut word = Word::literal(String::new());
            word.expands = true;
            word.runtime_unknown = true;
            state.unknown.push(word);
            state.exact = false;
            state
        });
        if let Some(prior) = array(scope, name) {
            if self.frontend.zsh
                || prior
                    .binding_values()
                    .iter()
                    .filter_map(BindingValue::known)
                    .any(|value| {
                        !matches!(
                            super::arithmetic::armed(value),
                            super::arithmetic::Arming::Inert
                        )
                    })
            {
                state.merge(&prior);
            }
        } else if let Some(prior) = scope.bindings.get(name)
            && (self.frontend.zsh
                || prior
                    .values
                    .iter()
                    .filter_map(BindingValue::known)
                    .any(|value| {
                        !matches!(
                            super::arithmetic::armed(value),
                            super::arithmetic::Arming::Inert
                        )
                    }))
        {
            state.merge(&IndexedArray::scalar(self.frontend.zsh, &prior.values));
        }
        store(scope, name, state);
    }

    pub(super) fn read_array_lines(
        &mut self,
        args: &[Word],
        targets: &[Redirect],
        scope: &mut Scope,
    ) {
        let mut trim = false;
        let mut name = "MAPFILE";
        let mut modeled = true;
        for word in args {
            if word == "-t" {
                trim = true;
            } else if super::statements::identifier(&word.text) {
                name = &word.text;
            } else {
                modeled = false;
            }
        }
        let mut state: Option<IndexedArray> = None;
        if modeled {
            for output in input(scope, targets).into_iter().flatten() {
                let mut candidate = IndexedArray::new(self.frontend.zsh);
                for line in output.split_inclusive('\n') {
                    candidate.push(vec![Word::literal(
                        if trim {
                            line.trim_end_matches('\n')
                        } else {
                            line
                        }
                        .into(),
                    )]);
                }
                if let Some(state) = &mut state {
                    state.merge(&candidate);
                } else {
                    state = Some(candidate);
                }
            }
        }
        if let Some(state) = state {
            let mut state = state;
            if let Some(prior) = array(scope, name).filter(|_| self.frontend.zsh) {
                state.merge(&prior);
            } else if let Some(prior) = scope.bindings.get(name).filter(|_| self.frontend.zsh) {
                state.merge(&IndexedArray::scalar(self.frontend.zsh, &prior.values));
            }
            store(scope, name, state);
        } else {
            self.read_array_fields(name, None, scope);
        }
    }
}

pub(super) fn input(scope: &Scope, targets: &[Redirect]) -> Option<Vec<String>> {
    if let Some(target) = targets.iter().rev().find(|target| {
        matches!(
            target.direction,
            Direction::In | Direction::Heredoc | Direction::Herestring
        )
    }) {
        match target.stream.as_deref() {
            Some(StreamOutput::Known(output)) => Some(output.clone()),
            Some(StreamOutput::Unknown) => None,
            None if matches!(target.direction, Direction::Heredoc | Direction::Herestring)
                && !target.expands =>
            {
                Some(vec![format!(
                    "{}{}",
                    target.target,
                    if target.direction == Direction::Herestring {
                        "\n"
                    } else {
                        ""
                    }
                )])
            }
            None if target.target == "/dev/null" && !target.expands => Some(vec![String::new()]),
            None => None,
        }
    } else {
        scope.pipeline_input.clone()
    }
}

fn indexed_piece(pieces: &[WordPieceWithSource], quoted: bool) -> Option<(&ParameterExpr, bool)> {
    let mut found = None;
    for piece in pieces {
        let parameter = match &piece.piece {
            WordPiece::ParameterExpansion(expr)
                if matches!(
                    words::parameter(expr),
                    Some(Parameter::NamedWithIndex { .. })
                ) =>
            {
                Some((expr, quoted))
            }
            WordPiece::DoubleQuotedSequence(inner) => indexed_piece(inner, true),
            _ => None,
        };
        if let Some(parameter) = parameter {
            if found.is_some() {
                return None;
            }
            found = Some(parameter);
        }
    }
    found
}

pub(super) fn expand(
    raw: &RawWord,
    scope: &mut Scope,
    evaluator: &mut Evaluator<'_, '_>,
    depth: usize,
) -> Result<Option<Vec<Expanded>>, CheckError> {
    if !matches!(raw.syntax, WordSyntax::Shell) || !raw.raw.contains('$') {
        return Ok(None);
    }
    let Ok(pieces) = word::parse(&raw.raw, &ParserOptions::default()) else {
        return Ok(None);
    };
    let plain = match pieces.as_slice() {
        [piece] => match &piece.piece {
            WordPiece::ParameterExpansion(expr) => Some((expr, false)),
            WordPiece::DoubleQuotedSequence(inner) => match inner.as_slice() {
                [piece] => match &piece.piece {
                    WordPiece::ParameterExpansion(expr) => Some((expr, true)),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        },
        _ => None,
    };
    let Some((expr, quoted)) = plain.or_else(|| indexed_piece(&pieces, false)) else {
        return Ok(None);
    };
    let Some(parameter) = words::parameter(expr) else {
        return Ok(None);
    };
    let (name, index, concatenate, all) = match parameter {
        Parameter::Named(name) => (name, None, quoted, evaluator.frontend.zsh),
        Parameter::NamedWithIndex { name, index } => (name, Some(index.as_str()), false, false),
        Parameter::NamedWithAllIndices { name, concatenate } => (name, None, *concatenate, true),
        _ => return Ok(None),
    };
    let Some(state) = array(scope, name) else {
        return Ok(None);
    };
    let affixes = if plain.is_none() {
        words::parameter_affixes(&raw.raw, name)
    } else {
        Some((String::new(), String::new(), !quoted))
    };
    let Some((prefix, suffix, _)) = affixes else {
        return Ok(None);
    };
    if !matches!(
        expr,
        ParameterExpr::Parameter {
            indirect: false,
            ..
        } | ParameterExpr::Substring {
            indirect: false,
            ..
        } | ParameterExpr::ParameterLength { .. }
    ) {
        return Ok(None);
    }
    // Preserve the ordinary word observer for code in indices and slice bounds.
    let contexts = scope.contexts();
    let observed = words::expand(
        &raw.raw,
        &raw.syntax,
        &words::ExpansionContext {
            variables: &contexts,
            runtime_variables: &BTreeSet::new(),
            pattern_variables: &scope.pattern_contexts(&contexts),
            host: evaluator.frontend.host,
            cwd: &scope.directory.current.render(),
            tilde_assigned: true,
        },
    )?;
    for expression in &observed.references {
        evaluator.armed_references(expression, scope, depth)?;
    }
    for code in &observed.nested {
        evaluator.isolated_source(code, scope, depth + 1)?;
    }
    evaluator.armed_reference(name, scope, depth)?;
    let mut results = Vec::new();
    for state in state
        .zsh
        .as_deref()
        .into_iter()
        .chain(std::iter::once(&state))
    {
        let all = all
            && (index.is_some() || !matches!(parameter, Parameter::Named(_)) || state.base == 1);
        if state.base == 1 && index.is_some_and(|index| integer(index, scope) == Some(0)) {
            continue;
        }
        let mut selected = if all {
            state.elements.values().cloned().collect::<Vec<_>>()
        } else if let Some(index) = index {
            if let Some(indices) = indices(index, scope) {
                let mut words = Vec::new();
                for index in indices {
                    if index < 0
                        && let Some((_, tail)) = &state.tail
                    {
                        union(
                            &mut words,
                            &state
                                .elements
                                .values()
                                .flatten()
                                .cloned()
                                .collect::<Vec<_>>(),
                        );
                        union(&mut words, tail);
                        continue;
                    }
                    let index = if index < 0 {
                        state.elements.keys().next_back().map_or(0, |last| last + 1) + index
                    } else {
                        index - state.base
                    };
                    union(
                        &mut words,
                        &state.elements.get(&index).cloned().unwrap_or_default(),
                    );
                    if !state.elements.contains_key(&index)
                        && let Some((start, tail)) = &state.tail
                        && index >= *start
                    {
                        union(&mut words, tail);
                    }
                }
                if words.is_empty() {
                    words.push(Word::literal(String::new()));
                }
                vec![words]
            } else {
                vec![
                    state
                        .elements
                        .values()
                        .flatten()
                        .chain(state.tail.as_ref().into_iter().flat_map(|(_, words)| words))
                        .cloned()
                        .collect(),
                ]
            }
        } else {
            vec![
                state
                    .elements
                    .get(&0)
                    .cloned()
                    .unwrap_or_else(|| vec![Word::literal(String::new())]),
            ]
        };
        if all && let Some((_, tail)) = &state.tail {
            selected.push(tail.clone());
        }
        if !state.unknown.is_empty() {
            if all {
                selected.push(state.unknown.clone());
            } else if let Some(words) = selected.first_mut() {
                union(words, &state.unknown);
            }
        }
        let mut unknown_count = !state.exact && all;
        if let ParameterExpr::Substring { offset, length, .. } = expr {
            let offset = integer(&offset.value, scope);
            let length = length.as_ref().map(|length| integer(&length.value, scope));
            if !state.exact && length.flatten() != Some(0) {
                selected = vec![selected.into_iter().flatten().collect()];
                unknown_count = length.flatten() != Some(1);
            } else if let Some(offset) =
                offset.filter(|_| length.is_none_or(|n| n.is_some_and(|n| n >= 0)))
            {
                let start = if offset < 0 {
                    selected
                        .len()
                        .saturating_sub(offset.unsigned_abs() as usize)
                } else {
                    offset as usize
                };
                selected = selected
                    .into_iter()
                    .skip(start)
                    .take(length.flatten().map_or(usize::MAX, |n| n as usize))
                    .collect();
            } else {
                selected = vec![selected.into_iter().flatten().collect()];
                unknown_count = length.flatten() != Some(1);
            }
        }
        if matches!(expr, ParameterExpr::ParameterLength { .. }) {
            let mut word = Word::literal(selected.len().to_string());
            word.runtime_unknown = unknown_count;
            word.expands = unknown_count;
            selected = vec![vec![word]];
            unknown_count = false;
        }
        let first = selected
            .iter()
            .filter_map(|words| words.first().cloned())
            .collect::<Vec<_>>();
        let mut sequences = vec![first.clone()];
        // Identity projections reuse the binding candidate union, never a product
        // of the alternatives at every index.
        for (index, words) in selected.iter().enumerate() {
            for word in words.iter().skip(1) {
                let mut sequence = first.clone();
                if let Some(slot) = sequence.get_mut(index) {
                    *slot = word.clone();
                    sequences.push(sequence);
                }
            }
        }
        for mut sequence in sequences {
            #[cfg(test)]
            {
                evaluator.output.array_words += sequence.len();
            }
            if concatenate && quoted {
                let delimiter = contexts.get("IFS").map_or_else(
                    || " ".into(),
                    |ifs| {
                        ifs.chars()
                            .next()
                            .map_or_else(String::new, |ch| ch.to_string())
                    },
                );
                let mut joined = snapshot(Word::literal(
                    sequence
                        .iter()
                        .map(Word::as_str)
                        .collect::<Vec<_>>()
                        .join(&delimiter),
                ));
                joined.expands = unknown_count || sequence.iter().any(|word| word.expands);
                joined.runtime_unknown = sequence.iter().any(|word| word.runtime_unknown);
                sequence = vec![joined];
            } else if !quoted {
                sequence = sequence
                    .into_iter()
                    .flat_map(|word| {
                        if word.expands || word.runtime_unknown {
                            return vec![word];
                        }
                        words::fields(&word.text, &[], contexts.get("IFS").map(String::as_str))
                            .into_iter()
                            .map(|text| word.with_text(text.into()))
                            .collect::<Vec<_>>()
                    })
                    .collect();
            }
            for word in &mut sequence {
                if !prefix.is_empty() || !suffix.is_empty() {
                    *word = snapshot(word.with_text(format!("{prefix}{}{suffix}", word.text)));
                    for range in &observed.word.quoted_ranges {
                        let right = range.end.min(prefix.len());
                        if range.start < right {
                            word.quoted_ranges.push(range.start..right);
                        }
                        let start = observed.word.text.len() - suffix.len();
                        let left = range.start.max(start);
                        if left < range.end {
                            let offset = word.text.len() - suffix.len();
                            word.quoted_ranges
                                .push(offset + left - start..offset + range.end - start);
                        }
                    }
                }
                word.cardinality_unknown |= unknown_count;
                word.field_count_unknown |= unknown_count;
                word.vars = observed.word.vars.clone();
                word.globs |= !quoted && word.text.contains(['*', '?', '[']);
            }
            let word = sequence
                .first()
                .cloned()
                .unwrap_or_else(|| Word::literal(String::new()));
            let expanded = Expanded {
                unknown_splitting: unknown_count,
                word,
                split: sequence,
                positional: true,
                nested: Vec::new(),
                arithmetic: Vec::new(),
                references: Vec::new(),
                tilde: false,
                parameters: Vec::new(),
                unsupported: false,
                lexical_ranges: Vec::new(),
            };
            if !results
                .iter()
                .any(|result: &Expanded| result.split == expanded.split)
            {
                results.push(expanded);
            }
        }
    }
    Ok(Some(results))
}

#[cfg(test)]
mod tests {
    #[test]
    fn unbounded_array_append_converges_by_resource_identity() {
        let count = |width, depth| {
            let fields = (0..width)
                .map(|n| format!("public{n}"))
                .collect::<Vec<_>>()
                .join(" ");
            let mut body = format!("A+=({fields});");
            for _ in 0..depth {
                body = format!("while test public; do {body} done;");
            }
            let output = crate::shell::observe(
                &format!("A=(prefix); {body} cat \"${{A[@]}}\""),
                crate::shell::Arm::Brush,
                "/synthetic/home",
                "/synthetic/project",
                true,
            )
            .unwrap();
            assert!(
                !output.gaps.contains(&crate::CoverageGap::InspectionBudget),
                "width={width}, depth={depth}"
            );
            let words = output
                .script
                .commands
                .iter()
                .filter(|c| c.argv.first().is_some_and(|w| w == "cat"))
                .flat_map(|c| c.argv.iter().skip(1))
                .map(|w| w.text.as_str())
                .collect::<std::collections::BTreeSet<_>>();
            assert!(words.contains("prefix"));
            for n in 0..width {
                assert!(words.contains(format!("public{n}").as_str()));
            }
            (output.statement_visits, output.array_words)
        };
        let widths = [2, 4, 8].map(|width| count(width, 2));
        let depths = [1, 2, 4, 8].map(|depth| count(4, depth));
        println!("array convergence widths={widths:?}, depths={depths:?}");
        for (small, large) in widths.iter().zip(widths.iter().skip(1)) {
            assert!(large.0 <= small.0 * 5 && large.1 <= small.1 * 5);
        }
        for (small, large) in depths.iter().zip(depths.iter().skip(1)) {
            assert!(large.0 <= small.0 * 4 && large.1 <= small.1 * 4);
        }
    }

    #[test]
    fn equal_array_executor_readings_share_one_projection() {
        let count = |expansion| {
            let output = crate::shell::observe(
                &format!("A=(first second); cat {expansion}"),
                crate::shell::Arm::Brush,
                "/synthetic/home",
                "/synthetic/project",
                true,
            )
            .unwrap();
            output
                .script
                .commands
                .iter()
                .filter(|command| command.argv.first().is_some_and(|word| word == "cat"))
                .count()
        };
        assert_eq!(count("\"${A[@]}\""), 1);
        assert_eq!(count("\"${A[1]}\""), 2);
        let output = crate::shell::observe(
            "A=(first second); for f in \"${A[@]}\"; do cat \"$f\"; done",
            crate::shell::Arm::Brush,
            "/synthetic/home",
            "/synthetic/project",
            true,
        )
        .unwrap();
        assert_eq!(
            output
                .script
                .commands
                .iter()
                .filter(|command| command.program.is_none())
                .count(),
            1
        );
    }

    #[test]
    fn array_repetition_projects_loop_candidates_once() {
        let count = |width| {
            let items = (0..width)
                .map(|n| format!("public{n}"))
                .collect::<Vec<_>>()
                .join(" ");
            let source = format!(
                "cd public && say() {{ printf '%s' public | cat; }} && A=({items}); for f in \"${{A[@]}}\"; do x=$(say | grep -oF \"$f\"); y=$(say | grep -oF \"$f\"); z=$(say | grep -oF \"$f\"); printf '%s' \"$f\"; done"
            );
            let output = crate::shell::observe(
                &source,
                crate::shell::Arm::Brush,
                "/synthetic/home",
                "/synthetic/project",
                true,
            )
            .unwrap();
            assert!(
                !output.gaps.contains(&crate::CoverageGap::InspectionBudget),
                "width={width}: {:?}",
                output.gaps
            );
            (
                output.statement_visits,
                output.source_entries,
                output.script.commands.len(),
            )
        };
        let counts = [4, 8, 16, 24, 32].map(count);
        println!("array repeated-loop work: {counts:?}");
        for (small, large) in counts.iter().zip(counts.iter().skip(1)) {
            assert!(
                small.0 > 0
                    && large.0 <= small.0 * 3
                    && large.1 == small.1
                    && large.2 <= small.2 * 3,
                "{counts:?}"
            );
        }
    }

    #[test]
    fn array_candidate_work_grows_polynomially() {
        let count = |width| {
            let items = (0..width)
                .map(|n| format!("public{n}"))
                .collect::<Vec<_>>()
                .join(" ");
            let source = format!(
                "A=(base); for f in {items}; do test -f public || A+=(\"$f\"); done; cat \"${{A[@]}}\""
            );
            let output = crate::shell::observe(
                &source,
                crate::shell::Arm::Brush,
                "/synthetic/home",
                "/synthetic/project",
                true,
            )
            .unwrap();
            assert!(
                !output.gaps.contains(&crate::CoverageGap::InspectionBudget),
                "width={width}: {:?}",
                output.gaps
            );
            let operands = output
                .script
                .commands
                .iter()
                .filter(|command| command.argv.first().is_some_and(|word| word == "cat"))
                .flat_map(|command| command.argv.iter().skip(1))
                .map(|word| word.text.as_str())
                .collect::<std::collections::BTreeSet<_>>();
            for n in 0..width {
                assert!(operands.contains(format!("public{n}").as_str()));
            }
            (
                output.array_words,
                output.candidate_pairs,
                output.statement_visits,
            )
        };
        let counts = [4, 8, 16].map(count);
        println!("array candidate work: {counts:?}");
        for (small, large) in counts.iter().zip(counts.iter().skip(1)) {
            assert!(
                small.0 > 0
                    && large.0 <= small.0 * 5
                    && large.1 <= small.1 * 5
                    && large.2 <= small.2 * 5,
                "{counts:?}"
            );
        }
    }
}
