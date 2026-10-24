use super::scoped::{expand_positional_argv, expand_process_input};
use super::*;

pub(super) fn expand_candidates(
    raw: &RawWord,
    scope: &mut statements::Scope,
    evaluator: &mut statements::Evaluator<'_, '_>,
    depth: usize,
) -> Result<(Vec<Expanded>, Vec<String>), CheckError> {
    if let WordSyntax::ProcessInput(input) = &raw.syntax {
        return expand_process_input(input, scope, evaluator, depth)
            .map(|words| (words, Vec::new()));
    }
    if let Some(arguments) = expand_positional_argv(raw, scope, evaluator) {
        return Ok((arguments, Vec::new()));
    }
    if let Some(elements) = arrays::expand(raw, scope, evaluator, depth)? {
        return Ok((elements, Vec::new()));
    }
    let first = scope.contexts();
    let cwd = scope.directory.current.render();
    let seed = seed_expansion(raw, scope, evaluator, &first, &cwd)?;
    let contexts = contexts::binding_contexts(&seed, scope, evaluator)?;
    let mut nested = nested_sources(raw, scope, evaluator);
    let mut result = Vec::new();
    let mut updates = std::collections::BTreeMap::<String, Vec<Update>>::new();
    let mut context = first;
    let mut hypotheses = std::collections::BTreeSet::new();
    let mut inherited = std::collections::BTreeSet::new();
    let mut unset = std::collections::BTreeSet::new();
    'expansions: for delta in contexts {
        #[cfg(test)]
        {
            evaluator.output.context_delta_entries += delta.len();
        }
        contexts::apply(
            delta,
            &mut context,
            &mut hypotheses,
            &mut inherited,
            &mut unset,
        );
        let mut unknown_variables = scope.unknown_parameters(&context);
        unknown_variables.retain(|name| !hypotheses.contains(name));
        let mut facts = scope.facts(&seed.word.vars, &context, &unknown_variables, &unset);
        facts.hypotheses.clone_from(&hypotheses);
        facts.inherited.clone_from(&inherited);
        {
            for zsh in std::iter::once(false).chain(
                (scope.zsh && (seed.modifiers || !seed.named_tildes.is_empty())).then_some(true),
            ) {
                {
                    for tilde_assigned in if seed.tilde && context.contains_key("PWD") {
                        vec![true, false]
                    } else {
                        vec![true]
                    } {
                        facts.alternate = false;
                        loop {
                            let runtime_variables = runtime_variables(&seed, scope, &context);
                            crate::check_deadline(evaluator.deadline)?;
                            if result.len() == 512 {
                                evaluator.output.gap(CoverageGap::InspectionBudget);
                                break 'expansions;
                            }
                            #[cfg(test)]
                            {
                                evaluator.output.word_candidates_max =
                                    evaluator.output.word_candidates_max.max(result.len() + 1);
                            }
                            let expanded = expand_reading(
                                raw,
                                scope,
                                evaluator,
                                &words::ExpansionContext {
                                    zsh,
                                    assignments: &[],
                                    variables: &context,
                                    unknown_variables: &unknown_variables,
                                    deadline: evaluator.deadline,
                                    runtime_variables: &runtime_variables,
                                    pattern_variables: &scope.pattern_contexts(&context),
                                    host: evaluator.frontend.host,
                                    cwd: &cwd,
                                    tilde_assigned,
                                    facts: &facts,
                                },
                            )?;
                            for code in &expanded.nested {
                                if !nested.contains(code) {
                                    nested.push(code.clone());
                                }
                            }
                            record_updates(&mut updates, &expanded, &context);
                            let again = !facts.alternate && expanded.uncertain_decisions;
                            result.push(expanded);
                            if !again {
                                break;
                            }
                            facts.alternate = true;
                        }
                    }
                }
            }
        }
    }
    apply_updates(scope, updates);
    Ok((result, nested))
}

mod contexts;

fn nested_sources(
    raw: &RawWord,
    scope: &statements::Scope,
    evaluator: &mut statements::Evaluator<'_, '_>,
) -> Vec<String> {
    let mut nested = Vec::new();
    for expansion in &raw.expansions {
        match expansion {
            RawExpansion::Code(code) => {
                if !nested.contains(code) {
                    nested.push(code.clone());
                }
            }
            RawExpansion::Variable(name) => {
                if let Some(binding) = scope.bindings.get(name) {
                    for value in binding.values.iter() {
                        if let statements::BindingValue::Known(code) = value {
                            if !nested.contains(code) {
                                nested.push(code.clone());
                            }
                        } else if value == &statements::BindingValue::Undetermined {
                            evaluator.output.gap(CoverageGap::UnsupportedShellSyntax);
                        }
                    }
                }
            }
        }
    }
    nested
}

fn seed_expansion(
    raw: &RawWord,
    scope: &statements::Scope,
    evaluator: &statements::Evaluator<'_, '_>,
    first: &std::collections::BTreeMap<String, String>,
    cwd: &str,
) -> Result<Box<Expanded>, CheckError> {
    let unknown_variables = scope.unknown_parameters(first);
    let seed = words::expand_boxed(
        &raw.raw,
        &raw.syntax,
        &words::ExpansionContext {
            zsh: false,
            assignments: &[],
            variables: first,
            unknown_variables: &unknown_variables,
            deadline: evaluator.deadline,
            runtime_variables: &std::collections::BTreeSet::new(),
            pattern_variables: &scope.pattern_contexts(first),
            host: evaluator.frontend.host,
            cwd,
            tilde_assigned: true,
            facts: &scope.facts(
                scope.bindings.keys(),
                first,
                &unknown_variables,
                &Default::default(),
            ),
        },
    )?;
    Ok(seed)
}

fn runtime_variables(
    seed: &Expanded,
    scope: &statements::Scope,
    context: &std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeSet<String> {
    let mut runtime_variables = seed
        .word
        .vars
        .iter()
        .filter(|name| {
            scope.bindings.get(*name).is_some_and(|binding| {
                binding.values.iter().any(|value| match value {
                    statements::BindingValue::RuntimeUnknown(Some(value)) => {
                        context.get(*name) == Some(&value.text)
                    }
                    statements::BindingValue::RuntimeUnknown(None) => !context.contains_key(*name),
                    _ => false,
                })
            })
        })
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    // The shell starts with the default IFS whatever it inherited, and an
    // unset IFS splits as the default does.
    if scope.bindings.get("IFS").is_some_and(|binding| {
        binding.values.iter().any(|value| {
            value.known().is_none()
                && !matches!(
                    value,
                    statements::BindingValue::Inherited | statements::BindingValue::Unset
                )
        })
    }) {
        runtime_variables.insert("IFS".into());
    }
    runtime_variables
}

fn annotate_bindings(
    raw: &RawWord,
    scope: &statements::Scope,
    context: &std::collections::BTreeMap<String, String>,
    expanded: &mut Expanded,
) {
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
    let repeated = expanded
        .word
        .vars
        .iter()
        .filter(|name| {
            scope.bindings.get(*name).is_some_and(|binding| {
                binding
                    .values
                    .iter()
                    .any(|value| matches!(value, statements::BindingValue::RepeatedFields(_)))
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    if !repeated.is_empty() {
        if repeated
            .iter()
            .any(|name| words::parameter_affixes(&raw.raw, name).is_none())
        {
            for word in std::iter::once(&mut expanded.word).chain(&mut expanded.split) {
                word.expands = true;
                word.gaps = vec![crate::record::Gap {
                    range: 0..word.text.len(),
                    nonempty: false,
                }];
            }
        }
        // The literal fields are known; their repetition count is not.
        // Consumers whose roles depend on position need the unknown count.
        expanded.word.cardinality_unknown = true;
        expanded.word.field_count_unknown = repeated
            .iter()
            .any(|name| words::parameter_affixes(&raw.raw, name).is_none_or(|(_, _, split)| split));
        // One quoted repetition keeps the literal text around it in every
        // projection.
        if let [name] = repeated.as_slice()
            && let Some((prefix, suffix, false)) = words::parameter_affixes(&raw.raw, name)
        {
            for word in std::iter::once(&mut expanded.word).chain(&mut expanded.split) {
                if word.text.len() >= prefix.len() + suffix.len()
                    && word.text.starts_with(&prefix)
                    && word.text.ends_with(&suffix)
                {
                    word.gaps = vec![crate::record::Gap {
                        range: prefix.len()..word.text.len() - suffix.len(),
                        nonempty: false,
                    }];
                }
            }
        }
        for word in &mut expanded.split {
            word.cardinality_unknown = true;
            word.field_count_unknown = expanded.word.field_count_unknown;
        }
    }
    if expanded.word.vars.iter().any(|name| {
        scope.bindings.get(name).is_some_and(|binding| {
            binding.values.iter().any(|value| {
                matches!(
                    value,
                    statements::BindingValue::ShellMatches(text)
                        | statements::BindingValue::ShellDerived(text)
                        if context.get(name) == Some(&text.text)
                )
            })
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
                    statements::BindingValue::RuntimeDerived(text)
                        | statements::BindingValue::ShellDerived(text)
                        if context.get(name) == Some(&text.text)
                )
            })
        })
    }) {
        expanded.word.expands = true;
        for word in &mut expanded.split {
            word.expands = true;
        }
    }
}

fn expand_reading(
    raw: &RawWord,
    scope: &mut statements::Scope,
    evaluator: &mut statements::Evaluator<'_, '_>,
    expansion: &words::ExpansionContext<'_>,
) -> Result<Expanded, CheckError> {
    let mut expanded = words::expand(&raw.raw, &raw.syntax, expansion)?;
    annotate_bindings(raw, scope, expansion.variables, &mut expanded);
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
    let coverage = WordCoverage {
        raw: raw.raw.clone(),
        parameters: expanded.parameters.clone(),
        unsupported: expanded.unsupported,
    };
    // Each reading of a word covers the same regions.
    if (coverage.unsupported || !coverage.parameters.is_empty())
        && !evaluator.output.word_coverage.contains(&coverage)
    {
        evaluator.output.word_coverage.push(coverage);
    }
    for expression in &expanded.arithmetic {
        for code in super::arithmetic::evaluate(expression)?.code {
            if !expanded.nested.contains(&code) {
                expanded.nested.push(code);
            }
        }
    }
    Ok(expanded)
}

struct Update {
    old: Option<String>,
    value: statements::BindingValue,
    /// The assignment happens whatever the old value was.
    definite: bool,
}

/// Records the assignments a reading of the word makes in `context`.
fn record_updates(
    updates: &mut std::collections::BTreeMap<String, Vec<Update>>,
    expanded: &Expanded,
    context: &std::collections::BTreeMap<String, String>,
) {
    for (name, value) in &expanded.assignments {
        updates.entry(name.clone()).or_default().push(Update {
            old: context.get(name).cloned(),
            value: statements::BindingValue::Known(value.clone()),
            definite: true,
        });
    }
    for (name, value) in &expanded.conditional_assignments {
        updates.entry(name.clone()).or_default().push(Update {
            old: context.get(name).cloned(),
            value: statements::BindingValue::RuntimeUnknown(Some(value.clone())),
            definite: false,
        });
    }
}

fn apply_updates(
    scope: &mut statements::Scope,
    updates: std::collections::BTreeMap<String, Vec<Update>>,
) {
    for (name, updates) in updates {
        let mut values = scope.bindings.get(&name).map_or_else(
            || vec![scope.absent()],
            |binding| binding.values.as_ref().clone(),
        );
        // A definite assignment replaces the value its context read, which
        // was no value for a name the context had unset.
        values.retain(|value| {
            !updates.iter().any(|update| {
                update.definite
                    && match value.known() {
                        Some(value) => Some(value) == update.old.as_ref(),
                        None => update.old.is_none() && value == &statements::BindingValue::Unset,
                    }
            })
        });
        // After a conditional default the name is set, and the update's
        // representative stands for both its old value and the default.
        if updates.iter().any(|update| !update.definite) {
            values.retain(|value| {
                !matches!(
                    value,
                    statements::BindingValue::Inherited
                        | statements::BindingValue::Unset
                        | statements::BindingValue::RuntimeUnknown(None)
                )
            });
        }
        for update in updates {
            if !values.contains(&update.value) {
                values.push(update.value);
            }
        }
        scope.assign(name, values);
    }
}
