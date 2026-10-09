use super::*;

pub(in crate::shell) fn update(scope: &mut Scope, name: &str, values: &mut Vec<BindingValue>) {
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

pub(in crate::shell) fn tied_cdpath(name: &str, values: &[BindingValue]) -> Vec<BindingValue> {
    let mut result = Vec::new();
    for value in values {
        if name == "CDPATH" {
            if let BindingValue::Known(value) = value {
                let mut array = IndexedArray::new(true);
                for entry in value.split(':') {
                    array.push(vec![Word::literal(entry.into())]);
                }
                result.push(BindingValue::Array(Box::new(array)));
            } else {
                result.push(BindingValue::RuntimeUnknown(None));
            }
        } else if let BindingValue::Array(array) = value {
            let state = array.zsh.as_deref().unwrap_or(array);
            if !state.exact || !state.unknown.is_empty() || state.tail.is_some() {
                result.push(BindingValue::RuntimeUnknown(None));
            }
            let mut candidates = vec![String::new()];
            for (index, words) in state.elements.values().enumerate() {
                let mut next = Vec::new();
                for prefix in &candidates {
                    for word in words {
                        if word.expands || word.runtime_unknown || word.globs || word.shell_matches
                        {
                            if !result.contains(&BindingValue::RuntimeUnknown(None)) {
                                result.push(BindingValue::RuntimeUnknown(None));
                            }
                            continue;
                        }
                        let joined =
                            format!("{prefix}{}{}", if index == 0 { "" } else { ":" }, word.text);
                        if !next.contains(&joined) {
                            if next.len() == 512 {
                                return vec![BindingValue::Undetermined];
                            }
                            next.push(joined);
                        }
                    }
                }
                candidates = next;
            }
            result.extend(candidates.into_iter().map(BindingValue::Known));
        } else {
            result.push(BindingValue::RuntimeUnknown(None));
        }
    }
    result
}

pub(in crate::shell) fn store(scope: &mut Scope, name: &str, state: IndexedArray) {
    let prefix = format!("{name}[");
    std::rc::Rc::make_mut(&mut scope.bindings)
        .retain(|key, _| key != name && !key.starts_with(&prefix));
    for (index, words) in &state.elements {
        let values = words
            .iter()
            .map(|word| {
                if word.runtime_unknown || word.expands {
                    BindingValue::RuntimeUnknown(Some(word.text.clone()))
                } else if word.globs || word.shell_matches {
                    BindingValue::ShellMatches(crate::shell::statements::ShellValue::from_word(
                        word,
                    ))
                } else {
                    BindingValue::Known(word.text.clone())
                }
            })
            .collect();
        scope.assign_binding(format!("{name}[{}]", index + state.base), values);
    }
    scope.assign(name.into(), vec![BindingValue::Array(Box::new(state))]);
}
