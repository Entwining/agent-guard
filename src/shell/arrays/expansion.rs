use super::*;
use brush_parser::{
    ParserOptions,
    word::{self, Parameter, ParameterExpr, WordPiece, WordPieceWithSource},
};

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

pub(in crate::shell) fn expand(
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
    let plain = plain_parameter(&pieces);
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
    let observed = observe_indices(raw, &contexts, scope, evaluator, depth)?;
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
        let selected = select_elements(state, index, all, scope);
        let (selected, unknown_count) = slice_elements(state, expr, scope, selected, all);
        let sequences = projections(&selected);
        for sequence in sequences {
            #[cfg(test)]
            {
                evaluator.output.array_words += sequence.len();
            }
            let expanded = expand_sequence(
                sequence,
                &contexts,
                concatenate,
                quoted,
                (&prefix, &suffix),
                &observed,
                unknown_count,
            );
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

fn plain_parameter(pieces: &[WordPieceWithSource]) -> Option<(&ParameterExpr, bool)> {
    match pieces {
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
    }
}

fn observe_indices(
    raw: &RawWord,
    contexts: &BTreeMap<String, String>,
    scope: &mut Scope,
    evaluator: &mut Evaluator<'_, '_>,
    depth: usize,
) -> Result<Expanded, CheckError> {
    let observed = words::expand(
        &raw.raw,
        &raw.syntax,
        &words::ExpansionContext {
            named_dirs: &BTreeMap::new(),
            zsh: false,
            assignments: &[],
            variables: contexts,
            unknown_variables: &scope.unknown_parameters(contexts),
            deadline: evaluator.deadline,
            runtime_variables: &BTreeSet::new(),
            pattern_variables: &scope.pattern_contexts(contexts),
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
    Ok(observed)
}

fn select_elements(
    state: &IndexedArray,
    index: Option<&str>,
    all: bool,
    scope: &Scope,
) -> Vec<Vec<Word>> {
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
    selected
}

fn slice_elements(
    state: &IndexedArray,
    expr: &ParameterExpr,
    scope: &Scope,
    mut selected: Vec<Vec<Word>>,
    all: bool,
) -> (Vec<Vec<Word>>, bool) {
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
    (selected, unknown_count)
}

fn projections(selected: &[Vec<Word>]) -> Vec<Vec<Word>> {
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
    sequences
}

fn expand_sequence(
    mut sequence: Vec<Word>,
    contexts: &BTreeMap<String, String>,
    concatenate: bool,
    quoted: bool,
    affixes: (&str, &str),
    observed: &Expanded,
    unknown_count: bool,
) -> Expanded {
    let (prefix, suffix) = affixes;
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
    Expanded {
        named_tildes: std::collections::BTreeSet::new(),
        modifiers: false,
        unset_parameters: std::collections::BTreeSet::new(),
        empty_parameters: std::collections::BTreeSet::new(),
        unknown_splitting: unknown_count,
        word,
        split: sequence,
        positional: true,
        nested: Vec::new(),
        arithmetic: Vec::new(),
        references: Vec::new(),
        assignments: Vec::new(),
        tilde: false,
        parameters: Vec::new(),
        unsupported: false,
        lexical_ranges: Vec::new(),
    }
}
