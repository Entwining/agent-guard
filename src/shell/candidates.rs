use super::scoped::{expand_positional_argv, expand_process_input};
use super::*;

pub(super) fn expand_candidates(
    raw: &RawWord,
    scope: &mut statements::Scope,
    evaluator: &mut statements::Evaluator<'_, '_>,
    depth: usize,
    observe_bindings: bool,
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
    let contexts = contexts::binding_contexts(&seed, scope, evaluator, depth, observe_bindings)?;
    let mut nested = nested_sources(raw, scope, evaluator);
    let mut result = Vec::new();
    let mut updates = std::collections::BTreeMap::<String, Vec<(Option<String>, String)>>::new();
    let mut context = first;
    let (named_contexts, named_bound) = scope.named_contexts(&seed.named_tildes);
    if named_bound {
        evaluator.output.gap(CoverageGap::InspectionBudget);
    }
    'expansions: for delta in contexts {
        #[cfg(test)]
        {
            evaluator.output.context_delta_entries += delta.len();
        }
        for (name, value) in delta {
            if let Some(value) = value {
                context.insert(name, value);
            } else {
                context.remove(&name);
            }
        }
        let (cwd_readings, pwd_from_cwd) = cwd_readings(&seed, scope, &context, &cwd);
        for cwd in cwd_readings {
            if pwd_from_cwd {
                context.insert("PWD".into(), cwd.clone());
            }
            for zsh in std::iter::once(false).chain(
                (scope.zsh && (seed.modifiers || !seed.named_tildes.is_empty())).then_some(true),
            ) {
                for named_dirs in &named_contexts {
                    for tilde_assigned in if seed.tilde && context.contains_key("PWD") {
                        vec![true, false]
                    } else {
                        vec![true]
                    } {
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
                            depth,
                            &words::ExpansionContext {
                                named_dirs,
                                zsh,
                                assignments: &[],
                                variables: &context,
                                unknown_variables: &scope.unknown_parameters(&context),
                                deadline: evaluator.deadline,
                                runtime_variables: &runtime_variables,
                                pattern_variables: &scope.pattern_contexts(&context),
                                host: evaluator.frontend.host,
                                cwd: &cwd,
                                tilde_assigned,
                            },
                        )?;
                        for code in &expanded.nested {
                            if !nested.contains(code) {
                                nested.push(code.clone());
                            }
                        }
                        for (name, value) in &expanded.assignments {
                            updates
                                .entry(name.clone())
                                .or_default()
                                .push((context.get(name).cloned(), value.clone()));
                        }
                        result.push(expanded);
                    }
                }
            }
        }
        if pwd_from_cwd {
            context.remove("PWD");
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
    let seed = words::expand_boxed(
        &raw.raw,
        &raw.syntax,
        &words::ExpansionContext {
            named_dirs: &std::collections::BTreeMap::new(),
            zsh: false,
            assignments: &[],
            variables: first,
            unknown_variables: &scope.unknown_parameters(first),
            deadline: evaluator.deadline,
            runtime_variables: &std::collections::BTreeSet::new(),
            pattern_variables: &scope.pattern_contexts(first),
            host: evaluator.frontend.host,
            cwd,
            tilde_assigned: true,
        },
    )?;
    Ok(seed)
}

fn cwd_readings(
    seed: &Expanded,
    scope: &statements::Scope,
    context: &std::collections::BTreeMap<String, String>,
    cwd: &str,
) -> (Vec<String>, bool) {
    let pwd_from_cwd = seed.modifiers
        && seed.word.vars.iter().any(|name| name == "PWD")
        && !context.contains_key("PWD");
    let cwd_readings = std::iter::once(cwd.to_owned())
        .chain(
            scope
                .directory
                .alternatives
                .iter()
                .filter(|_| pwd_from_cwd)
                .map(|path| path.render()),
        )
        .collect::<Vec<_>>();
    (cwd_readings, pwd_from_cwd)
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
                        context.get(*name) == Some(value)
                    }
                    statements::BindingValue::RuntimeUnknown(None) => !context.contains_key(*name),
                    _ => false,
                })
            })
        })
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    if scope
        .bindings
        .get("IFS")
        .is_some_and(|binding| binding.values.iter().any(|value| value.known().is_none()))
    {
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
        .collect::<Vec<_>>();
    if !repeated.is_empty() {
        if repeated
            .iter()
            .any(|name| words::parameter_affixes(&raw.raw, name).is_none())
        {
            expanded.word.expands = true;
            for word in &mut expanded.split {
                word.expands = true;
            }
        }
        // The literal fields are known; their repetition count is not.
        // Consumers whose roles depend on position need the unknown count.
        expanded.word.cardinality_unknown = true;
        expanded.word.field_count_unknown = repeated
            .iter()
            .any(|name| words::parameter_affixes(&raw.raw, name).is_none_or(|(_, _, split)| split));
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
                    statements::BindingValue::RuntimeDerived(_)
                        | statements::BindingValue::ShellDerived(_)
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
    depth: usize,
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
    if expanded.unsupported || !expanded.parameters.is_empty() {
        evaluator.output.word_coverage.push(WordCoverage {
            raw: raw.raw.clone(),
            parameters: expanded.parameters.clone(),
            unsupported: expanded.unsupported,
        });
    }
    for expression in &expanded.arithmetic {
        evaluator.armed_references(expression, scope, depth)?;
        for code in evaluator.arithmetic_code(expression, scope)? {
            if !expanded.nested.contains(&code) {
                expanded.nested.push(code);
            }
        }
    }
    for expression in &expanded.references {
        evaluator.armed_references(expression, scope, depth)?;
    }
    Ok(expanded)
}

fn apply_updates(
    scope: &mut statements::Scope,
    updates: std::collections::BTreeMap<String, Vec<(Option<String>, String)>>,
) {
    for (name, updates) in updates {
        let mut values = scope
            .bindings
            .get(&name)
            .map_or_else(Vec::new, |binding| binding.values.as_ref().clone());
        values.retain(|value| {
            !updates.iter().any(|(old, _)| {
                value
                    .known()
                    .is_some_and(|value| Some(value) == old.as_ref())
            })
        });
        for (_, value) in updates {
            let value = statements::BindingValue::Known(value);
            if !values.contains(&value) {
                values.push(value);
            }
        }
        scope.assign(name, values);
    }
}
