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
        conditional_assignments: Vec::new(),
        tilde: false,
        parameters: Vec::new(),
        unsupported: false,
        lexical_ranges: Vec::new(),
        uncertain_decisions: false,
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
            arguments = vec![join_arguments(&arguments)];
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
            conditional_assignments: Vec::new(),
            tilde: false,
            parameters: Vec::new(),
            unsupported: false,
            lexical_ranges: Vec::new(),
            uncertain_decisions: false,
        });
    }
    Some(results)
}

/// Quoted `$*`: the arguments joined by a space.
fn join_arguments(arguments: &[Word]) -> Word {
    let text = arguments
        .iter()
        .map(Word::as_str)
        .collect::<Vec<_>>()
        .join(" ");
    let mut joined = Word::literal(text);
    // Each argument's unknown runs keep their place in the joined text, so a
    // value sure to be nonempty stays so. The runtime separator is the first
    // IFS character, which the command may have changed or emptied, even
    // through a write to an unknown name, so each space stays an unknown run
    // that may be empty. A join of literal arguments stays literal, a known
    // limit in `docs/setup.md`, and an unknown argument count or repetition
    // leaves the flags below to make the whole word unknown.
    if arguments.iter().any(|argument| argument.exact().is_none())
        && arguments
            .iter()
            .all(|argument| !argument.field_count_unknown && !argument.cardinality_unknown)
    {
        let mut offset = 0;
        for argument in arguments {
            if offset > 0 {
                joined.gaps.push(crate::record::Gap {
                    range: offset - 1..offset,
                    nonempty: false,
                });
            }
            joined
                .gaps
                .extend(argument.unknown().iter().map(|gap| crate::record::Gap {
                    range: gap.range.start + offset..gap.range.end + offset,
                    nonempty: gap.nonempty,
                }));
            offset += argument.text.len() + 1;
        }
    }
    for argument in arguments {
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
    joined
}

pub(super) fn expand_scoped(
    raw: &RawWord,
    scope: &mut statements::Scope,
    evaluator: &mut statements::Evaluator<'_, '_>,
    depth: usize,
) -> Result<Vec<Expanded>, CheckError> {
    // Drop the candidate-building frame before recursively observing source;
    // its temporaries otherwise accumulate across the supported nesting depth.
    let (mut words, nested) = expand_candidates(raw, scope, evaluator, depth)?;
    let fields = words.iter().map(|word| word.split.len()).sum::<usize>();
    // Only repetition makes a value longer than the accepted input. A word may
    // still repeat a short value into a longer path.
    let oversized = scope.bindings.values().any(|binding| {
        binding.values.iter().any(|value| {
            value
                .lexical()
                .is_some_and(|text| text.len() > crate::limits::MAX_INPUT_BYTES)
        })
    });
    // Stored expansions feed later ones, so either quantity can double per
    // statement. The gap refuses the call; the remaining model only needs to
    // stay small.
    if fields > crate::limits::MAX_EXPANSION_FIELDS || oversized {
        evaluator.output.gap(CoverageGap::InspectionBudget);
        words.truncate(1);
        for word in &mut words {
            word.word = Word::literal(String::new());
            word.split.clear();
        }
    }
    #[cfg(test)]
    {
        evaluator.output.expansion_size += words
            .iter()
            .map(|word| word.split.len() + word.word.text.len())
            .sum::<usize>();
    }
    for code in nested {
        evaluator.isolated_source(&code, scope, depth + 1)?;
    }
    Ok(words)
}
