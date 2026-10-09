use super::super as shell;
use super::*;

pub(super) fn widen_runtime_repetition(prior: &Scope, next: &mut Scope) {
    let changes_values = |binding: &Binding, before: &Binding| {
        !Rc::ptr_eq(&binding.values, &before.values)
            && binding.values.iter().any(|value| {
                matches!(
                    value,
                    BindingValue::Array(_)
                        | BindingValue::RuntimeUnknown(_)
                        | BindingValue::RuntimeDerived(_)
                        | BindingValue::ShellDerived(_)
                )
            })
    };
    if Rc::ptr_eq(&prior.bindings, &next.bindings)
        || !next.bindings.iter().any(|(name, binding)| {
            prior
                .bindings
                .get(name)
                .is_some_and(|before| changes_values(binding, before))
        })
    {
        return;
    }
    let mut arrays = Vec::new();
    for (name, binding) in Rc::make_mut(&mut next.bindings) {
        let Some(before) = prior.bindings.get(name) else {
            continue;
        };
        if !changes_values(binding, before) {
            continue;
        }
        for value in Rc::make_mut(&mut binding.values) {
            if let BindingValue::Array(array) = value {
                let mut changed = false;
                for old in before.values.iter() {
                    if let BindingValue::Array(old) = old {
                        changed |= array.widen_append(old);
                    }
                }
                if changed && !arrays.contains(name) {
                    arrays.push(name.clone());
                }
                continue;
            }
            if !matches!(
                value,
                BindingValue::RuntimeUnknown(_)
                    | BindingValue::RuntimeDerived(_)
                    | BindingValue::ShellDerived(_)
            ) {
                continue;
            }
            let Some(text) = value.lexical() else {
                continue;
            };
            for old in before.values.iter() {
                if old.known().is_some() {
                    continue;
                }
                let Some(old) = old.lexical() else {
                    continue;
                };
                let Some(tail) = text.strip_prefix(old).filter(|tail| !tail.is_empty()) else {
                    continue;
                };
                if let Some(prefix) = old.strip_suffix(tail) {
                    // A repeated unknown fragment denotes arbitrary repetitions,
                    // not additional runtime evidence. Keep its fixed prefix and
                    // suffix; pathname matching must cover the widened middle.
                    *value = if tail.split('/').any(|part| part == "..") {
                        // Repeated parent traversal can escape the fixed prefix.
                        // A pathname pattern cannot represent normalization here.
                        BindingValue::Undetermined
                    } else if tail.contains('/') {
                        let prefix = prefix.strip_suffix("*/**/*").unwrap_or(prefix);
                        BindingValue::ShellDerived(format!("{prefix}*/**/*{tail}").into())
                    } else {
                        BindingValue::ShellDerived(
                            format!("{}*{tail}", prefix.trim_end_matches('*')).into(),
                        )
                    };
                    break;
                }
            }
        }
    }
    for name in arrays {
        if let Some(array) = shell::arrays::array(next, &name) {
            shell::arrays::store(next, &name, array);
        }
    }
}
