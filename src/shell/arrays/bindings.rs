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
