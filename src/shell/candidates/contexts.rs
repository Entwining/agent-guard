use super::*;

pub(super) fn binding_contexts(
    seed: &Expanded,
    scope: &mut statements::Scope,
    evaluator: &mut statements::Evaluator<'_, '_>,
) -> Result<Vec<std::collections::BTreeMap<String, Option<String>>>, CheckError> {
    let mut contexts = vec![std::collections::BTreeMap::<String, Option<String>>::new()];
    let mut names = seed
        .word
        .vars
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    if !names.is_empty() && scope.bindings.contains_key("IFS") {
        names.insert("IFS".into());
    }
    for name in &names {
        if name.parse::<usize>().is_ok()
            && scope.bindings.get("#").is_some_and(|binding| {
                binding
                    .values
                    .iter()
                    .any(|value| matches!(value, statements::BindingValue::RuntimeDerived(_)))
            })
        {
            evaluator.output.gap(CoverageGap::UnsupportedShellSyntax);
        }
        contexts = project_binding(seed, scope, evaluator, name, contexts);
    }
    Ok(contexts)
}

fn project_binding(
    seed: &Expanded,
    scope: &statements::Scope,
    evaluator: &mut statements::Evaluator<'_, '_>,
    name: &String,
    mut contexts: Vec<std::collections::BTreeMap<String, Option<String>>>,
) -> Vec<std::collections::BTreeMap<String, Option<String>>> {
    if let Some(binding) = scope.bindings.get(name) {
        let mut next = Vec::new();
        for context in &contexts {
            for value in binding.values.iter() {
                // Preserve target inference from present lexical candidates;
                // absence at a join is runtime data, not a scope refusal.
                if !(seed.unset_parameters.contains(name) || seed.modifiers && name == "PWD")
                    && matches!(value, statements::BindingValue::RuntimeUnknown(Some(value)) if value.is_empty())
                    && binding.values.iter().any(|value| {
                        !matches!(value, statements::BindingValue::RuntimeUnknown(Some(value)) if value.is_empty())
                    })
                {
                    continue;
                }
                if let statements::BindingValue::RepeatedFields(repetition) = value {
                    for projection in repetition.projections() {
                        let mut context = context.clone();
                        context.insert(name.clone(), Some(projection));
                        if !next.contains(&context) {
                            if next.len() == 512 {
                                evaluator.output.gap(CoverageGap::InspectionBudget);
                                break;
                            }
                            next.push(context);
                        }
                    }
                    continue;
                }
                let mut context = context.clone();
                if (seed.unset_parameters.contains(name) || seed.modifiers && name == "PWD")
                    && matches!(value, statements::BindingValue::RuntimeUnknown(Some(value)) if value.is_empty())
                {
                    context.insert(name.clone(), None);
                } else if let Some(value) = value.lexical() {
                    context.insert(name.clone(), Some(value.clone()));
                } else {
                    context.insert(name.clone(), None);
                    if value == &statements::BindingValue::Undetermined {
                        evaluator.output.gap(CoverageGap::UnsupportedShellSyntax);
                    }
                }
                if !next.contains(&context) {
                    if next.len() == 512 {
                        evaluator.output.gap(CoverageGap::InspectionBudget);
                        break;
                    }
                    next.push(context);
                }
            }
            if seed.unset_parameters.contains(name)
                && (seed.empty_parameters.contains(name)
                    || scope.bindings.contains_key("IFS")
                    || binding.values.iter().any(|value| {
                        matches!(
                            value,
                            statements::BindingValue::RuntimeDerived(_)
                                | statements::BindingValue::ShellDerived(_)
                        )
                    }))
                && binding.values.iter().any(|value| {
                    matches!(
                        value,
                        statements::BindingValue::RuntimeUnknown(_)
                            | statements::BindingValue::RuntimeDerived(_)
                            | statements::BindingValue::ShellDerived(_)
                    )
                })
            {
                let mut empty = context.clone();
                empty.insert(name.clone(), Some(String::new()));
                if !next.contains(&empty) {
                    if next.len() == 512 {
                        evaluator.output.gap(CoverageGap::InspectionBudget);
                    } else {
                        next.push(empty);
                    }
                }
            }
        }
        contexts = next;
    }
    contexts
}
