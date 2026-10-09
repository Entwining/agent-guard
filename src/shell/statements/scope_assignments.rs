use super::super as shell;
use super::*;

impl Scope {
    pub(super) fn expanded_binding(&self, word: &crate::record::Word, text: &str) -> BindingValue {
        let raw = if text != word.text {
            assignment(&word.raw).map_or(word.raw.as_str(), |(_, value)| value)
        } else {
            &word.raw
        };
        for name in &word.vars {
            if let Some(binding) = self.bindings.get(name) {
                for value in binding.values.iter() {
                    if let BindingValue::RepeatedFields(repeated) = value
                        && let Some((prefix, suffix, _)) =
                            shell::words::parameter_affixes(raw, name)
                        && word.binding_candidates.get(name).is_some_and(|value| {
                            repeated.projections().contains(value)
                                && text == format!("{prefix}{value}{suffix}")
                        })
                    {
                        let mut repeated = repeated.clone();
                        repeated.prefix = format!("{prefix}{}", repeated.prefix);
                        repeated.suffix.push_str(&suffix);
                        return BindingValue::RepeatedFields(repeated);
                    }
                }
            }
        }
        if word.cardinality_unknown
            && shell::words::parameter_affixes(raw, word.vars.first().map_or("", String::as_str))
                .is_none()
        {
            return BindingValue::RuntimeDerived(text.into());
        }
        if word.shell_matches {
            return if word.globs || word.expands {
                BindingValue::ShellDerived(ShellValue::from_word(&word.with_text(text.into())))
            } else {
                BindingValue::ShellMatches(ShellValue::from_word(&word.with_text(text.into())))
            };
        }
        let candidates = word.vars.iter().filter_map(|name| self.bindings.get(name));
        let mut runtime = false;
        let mut representative = false;
        let mut derived = false;
        for binding in candidates {
            for value in binding.values.iter() {
                match value {
                    BindingValue::RuntimeDerived(_) => derived = true,
                    BindingValue::RuntimeUnknown(Some(value)) => {
                        runtime |= !value.is_empty()
                            || !binding.values.iter().any(|value| value.known().is_some());
                        representative |= value == text;
                    }
                    BindingValue::RuntimeUnknown(None) => {
                        runtime = true;
                    }
                    _ => {}
                }
            }
        }
        if derived || (runtime && (word.globs || !representative)) {
            // Repetition of unknown read fields is not new lexical evidence.
            // Widen only placeholder-only loop aggregates; any literal path,
            // arithmetic syntax or code keeps its full candidate text.
            if !self.loops.is_empty() {
                let mut fields = Vec::new();
                let mut placeholders = true;
                for field in text.split_whitespace() {
                    let name = field
                        .strip_prefix("${")
                        .and_then(|name| name.strip_suffix('}'))
                        .or_else(|| field.strip_prefix('$'));
                    placeholders &= name.is_some_and(identifier);
                    if !fields.contains(&field) {
                        fields.push(field);
                    }
                }
                if placeholders && !fields.is_empty() {
                    return BindingValue::RuntimeDerived(fields.join(" "));
                }
            }
            BindingValue::RuntimeDerived(text.into())
        } else if runtime || word.expands {
            BindingValue::RuntimeUnknown(Some(text.into()))
        } else {
            BindingValue::Known(text.into())
        }
    }
    pub(in crate::shell) fn local(&mut self, name: &str) {
        if let Some(frame) = Rc::make_mut(&mut self.frames).last_mut() {
            let frame = Rc::make_mut(frame);
            frame
                .entry(name.into())
                .or_insert_with(|| self.bindings.get(name).cloned());
            let prefix = format!("{name}[");
            let indexed = self
                .bindings
                .keys()
                .filter(|key| key.starts_with(&prefix))
                .cloned()
                .collect::<Vec<_>>();
            for key in indexed {
                frame
                    .entry(key.clone())
                    .or_insert_with(|| self.bindings.get(&key).cloned());
                Rc::make_mut(&mut self.bindings).remove(&key);
            }
            if self.bindings.get(name).is_some_and(|binding| {
                binding
                    .values
                    .iter()
                    .any(|value| matches!(value, BindingValue::Array(_)))
            }) {
                Rc::make_mut(&mut self.bindings).remove(name);
            }
        }
    }
    pub(in crate::shell) fn assign(&mut self, name: String, mut values: Vec<BindingValue>) {
        shell::arrays::update(self, &name, &mut values);
        let tied = self.zsh.then(|| cdpath_alias(&name)).flatten();
        let mirror = tied.map(|alias| (alias, shell::arrays::tied_cdpath(&name, &values)));
        if let Some((alias, mut mirrored)) = mirror {
            let prior = self.bindings.get(alias).cloned();
            if let Some(frame) = Rc::make_mut(&mut self.frames)
                .iter_mut()
                .rev()
                .find(|frame| frame.contains_key(&name))
            {
                Rc::make_mut(frame)
                    .entry(alias.into())
                    .or_insert_with(|| prior.clone());
            }
            let bash = prior.map_or_else(
                || vec![BindingValue::RuntimeUnknown(Some(String::new()))],
                |binding| {
                    binding.bash_values.map_or_else(
                        || shell::arrays::bash_only(Rc::unwrap_or_clone(binding.values)),
                        |values| values.as_ref().clone(),
                    )
                },
            );
            for value in &bash {
                if !mirrored.contains(value) {
                    mirrored.push(value.clone());
                }
            }
            self.assign_binding(name, values);
            self.assign_binding(alias.into(), mirrored);
            if let Some(binding) = Rc::make_mut(&mut self.bindings).get_mut(alias) {
                binding.bash_values = Some(std::rc::Rc::new(bash));
            }
        } else {
            self.assign_binding(name, values);
        }
    }
    pub(in crate::shell) fn assign_binding(&mut self, name: String, values: Vec<BindingValue>) {
        if let Some((base, _)) = name.split_once('[')
            && let Some(frame) = Rc::make_mut(&mut self.frames)
                .iter_mut()
                .rev()
                .find(|frame| frame.contains_key(base))
        {
            Rc::make_mut(frame)
                .entry(name.clone())
                .or_insert_with(|| self.bindings.get(&name).cloned());
        }
        let origins = if self.channels.is_empty() {
            None
        } else {
            let guards = if self.flow_end {
                Vec::new()
            } else {
                vec![self.flow_guard.as_ref().clone()]
            };
            let mut origins = Origins::new();
            for value in &values {
                let candidates = match value {
                    BindingValue::Array(array) => array.binding_values(),
                    value => vec![value.clone()],
                };
                for value in candidates {
                    if let Some(value) = value.known() {
                        origins.insert(value.clone(), guards.clone());
                    }
                }
            }
            (!origins.is_empty()).then(|| Rc::new(origins))
        };
        let exported = self
            .bindings
            .get(&name)
            .is_some_and(|binding| binding.exported);
        let arithmetic = self
            .bindings
            .get(&name)
            .is_some_and(|binding| binding.arithmetic);
        Rc::make_mut(&mut self.bindings).insert(
            name,
            Binding {
                #[cfg(test)]
                copies: EntryCopies::default(),
                origins,
                bash_values: None,
                values: Rc::new(values),
                exported,
                arithmetic,
            },
        );
    }
}
