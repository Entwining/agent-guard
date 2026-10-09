use super::*;

pub(super) fn expand_process_input(
    input: &ProcessInput,
    scope: &mut statements::Scope,
    evaluator: &mut statements::Evaluator<'_, '_>,
    depth: usize,
) -> Result<Vec<Expanded>, CheckError> {
    let mut inner = scope.isolated();
    let channel = evaluator.flow.channel();
    inner.capture(channel);
    let mut outputs = evaluator.run(
        &input.body,
        &mut inner,
        depth + 1,
        evaluator.output.parse_successes,
        true,
    )?;
    let output = outputs.remove(&channel).unwrap_or_default();
    let candidates = evaluator.flow.candidates(&output)?;
    let known = candidates
        .iter()
        .filter(|value| value.known)
        .map(|value| value.text.clone())
        .collect();
    let unknown = candidates.iter().any(|value| value.unknown);
    let mut word = Word::literal(format!("{}__observed_stream__", input.prefix));
    word.stream = Some(Box::new(crate::record::StreamOutput {
        value: output,
        known,
        unknown,
    }));
    Ok(vec![Expanded {
        named_tildes: std::collections::BTreeSet::new(),
        modifiers: false,
        unset_parameters: std::collections::BTreeSet::new(),
        empty_parameters: std::collections::BTreeSet::new(),
        unknown_splitting: false,
        split: vec![word.clone()],
        word,
        positional: false,
        nested: Vec::new(),
        arithmetic: Vec::new(),
        references: Vec::new(),
        assignments: Vec::new(),
        tilde: false,
        parameters: Vec::new(),
        unsupported: false,
        lexical_ranges: Vec::new(),
    }])
}

pub(super) fn expand_positional_argv(
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
            named_tildes: std::collections::BTreeSet::new(),
            modifiers: false,
            unset_parameters: std::collections::BTreeSet::new(),
            empty_parameters: std::collections::BTreeSet::new(),
            unknown_splitting: false,
            word,
            split: arguments,
            positional: true,
            nested: Vec::new(),
            arithmetic: Vec::new(),
            references: Vec::new(),
            assignments: Vec::new(),
            tilde: false,
            parameters: Vec::new(),
            unsupported: false,
            lexical_ranges: Vec::new(),
        });
    }
    Some(results)
}

pub(super) fn expand_scoped(
    raw: &RawWord,
    scope: &mut statements::Scope,
    evaluator: &mut statements::Evaluator<'_, '_>,
    depth: usize,
    observe_bindings: bool,
) -> Result<Vec<Expanded>, CheckError> {
    // Drop the candidate-building frame before recursively observing source;
    // its temporaries otherwise accumulate across the supported nesting depth.
    let (words, nested) = expand_candidates(raw, scope, evaluator, depth, observe_bindings)?;
    for code in nested {
        evaluator.isolated_source(&code, scope, depth + 1)?;
    }
    Ok(words)
}
