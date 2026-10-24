use super::*;

/// A name's state in one projected binding context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Projected {
    /// Known text, or the representative of a value the scope does not know.
    Value(String),
    /// Text assumed for an unknown value; what the context computes from it
    /// is evidence of one possible runtime text.
    Hypothesis(String),
    /// No text: an unknown value without a representative, or an unbound
    /// name, which keeps the reading the scope gives it.
    Absent,
    /// A name whose presence is unknown.
    Inherited,
    Unset,
    /// An inherited name assumed to be unset.
    UnsetHypothesis,
}

type Delta = std::collections::BTreeMap<String, Projected>;

/// Applies one projected context's changes to the names' text and to the
/// sets of names that are hypotheses, inherited and unset.
pub(super) fn apply(
    delta: Delta,
    context: &mut std::collections::BTreeMap<String, String>,
    hypotheses: &mut std::collections::BTreeSet<String>,
    inherited: &mut std::collections::BTreeSet<String>,
    unset: &mut std::collections::BTreeSet<String>,
) {
    for (name, value) in delta {
        hypotheses.remove(&name);
        inherited.remove(&name);
        unset.remove(&name);
        match value {
            Projected::Value(value) => {
                context.insert(name, value);
            }
            Projected::Hypothesis(value) => {
                context.insert(name.clone(), value);
                hypotheses.insert(name);
            }
            Projected::Absent => {
                context.remove(&name);
            }
            Projected::Inherited => {
                context.remove(&name);
                inherited.insert(name);
            }
            Projected::Unset => {
                context.remove(&name);
                unset.insert(name);
            }
            Projected::UnsetHypothesis => {
                context.remove(&name);
                unset.insert(name.clone());
                hypotheses.insert(name);
            }
        }
    }
}

/// Assumptions about values the scope does not know. Each enumerated
/// context gets one companion per kind, making that assumption for every
/// name that admits it: one context per name would double the contexts with
/// each independent name. Mixtures across names are not enumerated.
#[derive(Default)]
struct Assumptions {
    /// Names that may hold whatever the shell inherited beside the values
    /// the scope assigned them.
    inherited: Delta,
    /// Names whose inherited value may be unset.
    unset: Delta,
    /// Names whose unknown value may be empty.
    empty: Delta,
}

pub(super) fn binding_contexts(
    seed: &Expanded,
    scope: &mut statements::Scope,
    evaluator: &mut statements::Evaluator<'_, '_>,
) -> Result<Vec<Delta>, CheckError> {
    let mut contexts = vec![Delta::new()];
    let mut assumptions = Assumptions::default();
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
        contexts = project_binding(seed, scope, evaluator, name, contexts, &mut assumptions);
    }
    let enumerated = contexts.len();
    for assumption in [assumptions.inherited, assumptions.unset, assumptions.empty] {
        if assumption.is_empty() {
            continue;
        }
        for index in 0..enumerated {
            let mut companion = contexts[index].clone();
            companion.extend(assumption.clone());
            if !push(evaluator, &mut contexts, companion) {
                return Ok(contexts);
            }
        }
    }
    Ok(contexts)
}

fn project_binding(
    seed: &Expanded,
    scope: &statements::Scope,
    evaluator: &mut statements::Evaluator<'_, '_>,
    name: &String,
    contexts: Vec<Delta>,
    assumptions: &mut Assumptions,
) -> Vec<Delta> {
    let may_unset = seed.unset_parameters.contains(name) && !scope.inherits_unset();
    let Some(binding) = scope.bindings.get(name) else {
        if !may_unset || name == "PWD" {
            return contexts;
        }
        // An unbound name is inherited. Every context names it, so that
        // applying a context after the unset companion restores it.
        assumptions
            .unset
            .insert(name.clone(), Projected::UnsetHypothesis);
        return project(evaluator, name, &contexts, &[Projected::Absent]);
    };
    // An inherited value is any text or none. A runtime value with no known
    // text already spans that, unless the word tells unset from empty.
    let inherited = !scope.inherits_unset()
        && binding
            .values
            .contains(&statements::BindingValue::Inherited)
        && (seed.unset_parameters.contains(name)
            || !binding.values.iter().any(|value| {
                matches!(value, statements::BindingValue::RuntimeUnknown(_)) && {
                    let evidence = value.evidence();
                    matches!(evidence.gaps.as_slice(),
                        [gap] if gap.range == (0..evidence.text.len()) && !gap.nonempty)
                }
            }));
    let mut readings = Vec::new();
    for value in binding.values.iter() {
        match value {
            statements::BindingValue::RepeatedFields(repetition) => {
                readings.extend(repetition.projections().into_iter().map(Projected::Value));
            }
            statements::BindingValue::Known(value) => {
                readings.push(Projected::Value(value.clone()))
            }
            statements::BindingValue::Unset => readings.push(Projected::Unset),
            statements::BindingValue::Inherited if scope.inherits_unset() => {
                readings.push(Projected::Unset);
            }
            statements::BindingValue::Inherited => {}
            value => readings.push(match value.lexical() {
                Some(value) => Projected::Value(value.clone()),
                None => {
                    if value == &statements::BindingValue::Undetermined {
                        evaluator.output.gap(CoverageGap::UnsupportedShellSyntax);
                    }
                    Projected::Absent
                }
            }),
        }
    }
    if inherited {
        if readings.is_empty() {
            readings.push(Projected::Inherited);
        } else {
            assumptions
                .inherited
                .insert(name.clone(), Projected::Inherited);
        }
        if may_unset {
            assumptions
                .unset
                .insert(name.clone(), Projected::UnsetHypothesis);
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
        assumptions
            .empty
            .insert(name.clone(), Projected::Hypothesis(String::new()));
    }
    project(evaluator, name, &contexts, &readings)
}

/// Each context with each reading of `name`.
fn project(
    evaluator: &mut statements::Evaluator<'_, '_>,
    name: &str,
    contexts: &[Delta],
    readings: &[Projected],
) -> Vec<Delta> {
    let mut next = Vec::new();
    for context in contexts {
        for reading in readings {
            let mut context = context.clone();
            context.insert(name.to_owned(), reading.clone());
            if !push(evaluator, &mut next, context) {
                return next;
            }
        }
    }
    next
}

fn push(
    evaluator: &mut statements::Evaluator<'_, '_>,
    next: &mut Vec<Delta>,
    context: Delta,
) -> bool {
    if !next.contains(&context) {
        if next.len() == 512 {
            evaluator.output.gap(CoverageGap::InspectionBudget);
            return false;
        }
        next.push(context);
    }
    true
}
