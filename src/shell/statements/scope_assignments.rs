use super::super as shell;
use super::*;

impl Scope {
    /// The binding a word assigns, where `text` is the word's value after
    /// any `NAME=` prefix. Only an exact value is known; any other value
    /// keeps the word's text evidence.
    pub(super) fn expanded_binding(&self, word: &crate::record::Word, text: &str) -> BindingValue {
        let (raw, value) = if text != word.text {
            let start = word.text.len() - text.len();
            (
                assignment(&word.raw).map_or(word.raw.as_str(), |(_, value)| value),
                std::borrow::Cow::Owned(word.suffix(start)),
            )
        } else {
            (word.raw.as_str(), std::borrow::Cow::Borrowed(word))
        };
        for name in &word.vars {
            if let Some(binding) = self.bindings.get(name) {
                for candidate in binding.values.iter() {
                    if let BindingValue::RepeatedFields(repeated) = candidate
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
            return BindingValue::RuntimeDerived(ShellValue::from_word(&value));
        }
        if word.shell_matches {
            return if word.globs || word.expands {
                BindingValue::ShellDerived(ShellValue::from_word(&value))
            } else {
                BindingValue::ShellMatches(ShellValue::from_word(&value))
            };
        }
        if let Some(text) = value.exact() {
            return BindingValue::Known(text.into());
        }
        let candidates = word.vars.iter().filter_map(|name| self.bindings.get(name));
        let mut runtime = false;
        let mut representative = false;
        let mut derived = false;
        for binding in candidates {
            for candidate in binding.values.iter() {
                match candidate {
                    BindingValue::RuntimeDerived(_) => derived = true,
                    BindingValue::RuntimeUnknown(Some(candidate)) => {
                        runtime |= !candidate.is_empty()
                            || !binding.values.iter().any(|value| value.known().is_some());
                        representative |= candidate.text == text;
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
                    return BindingValue::RuntimeDerived(fields.join(" ").into());
                }
            }
            BindingValue::RuntimeDerived(ShellValue::from_word(&value))
        } else {
            BindingValue::RuntimeUnknown(Some(ShellValue::from_word(&value)))
        }
    }
    /// A mutation the shell has not established: `name` keeps its prior
    /// state and may also hold `values`. Returns false for an array, whose
    /// elements this update cannot weaken.
    pub(in crate::shell) fn possibly_assign(
        &mut self,
        name: &str,
        values: &[BindingValue],
    ) -> bool {
        let mut combined = self.bindings.get(name).map_or_else(
            || vec![self.absent()],
            |binding| binding.values.as_ref().clone(),
        );
        if combined
            .iter()
            .any(|value| matches!(value, BindingValue::Array(_)))
        {
            return false;
        }
        for value in values {
            if !combined.contains(value) {
                combined.push(value.clone());
            }
        }
        self.assign_binding(name.into(), combined);
        true
    }
    /// A successful unset, which also removes the variable's attributes.
    pub(in crate::shell) fn unset_binding(&mut self, name: &str) {
        self.assign_binding(name.into(), vec![BindingValue::Unset]);
        if let Some(binding) = Rc::make_mut(&mut self.bindings).get_mut(name) {
            binding.exported = Export::No;
            binding.arithmetic = false;
        }
    }
    /// An unset that may not happen: the variable keeps its state and
    /// attributes or loses both. A kept arithmetic attribute stays the
    /// stricter reading.
    pub(in crate::shell) fn possibly_unset(&mut self, name: &str) -> bool {
        let complete = self.possibly_assign(name, &[BindingValue::Unset]);
        if let Some(binding) = Rc::make_mut(&mut self.bindings).get_mut(name) {
            binding.exported = binding.exported.join(Export::No);
        }
        complete
    }
    /// A mutation of a variable the shell has not identified: every tracked
    /// variable keeps its prior state and may also hold `value` or have
    /// changed whether it is exported. Returns false when an array binding
    /// cannot carry it.
    pub(in crate::shell) fn may_mutate(&mut self, value: &BindingValue) -> bool {
        let mut complete = true;
        for (name, binding) in Rc::make_mut(&mut self.bindings).iter_mut() {
            if !identifier(name) {
                continue;
            }
            binding.exported = binding.exported.join(Export::Unknown);
            if binding
                .values
                .iter()
                .any(|value| matches!(value, BindingValue::Array(_)))
            {
                complete = false;
            } else if !binding.values.contains(value) {
                Rc::make_mut(&mut binding.values).push(value.clone());
            }
        }
        if value != &BindingValue::Unset {
            self.inherited_unset = false;
        }
        complete
    }
    /// The positional parameters after a command that may have replaced
    /// them with unknown ones. Untracked parameters are already unknown.
    pub(in crate::shell) fn positionals_unknown(&mut self) {
        let Some(binding) = self.bindings.get("@") else {
            return;
        };
        let mut word = crate::record::Word::literal("${@}".into());
        word.expands = true;
        word.runtime_unknown = true;
        word.field_count_unknown = true;
        word.vars.push("@".into());
        let mut values = binding.values.as_ref().clone();
        let unknown = BindingValue::Arguments(vec![word]);
        if !values.contains(&unknown) {
            values.push(unknown);
        }
        self.assign_binding("@".into(), values);
        for (name, binding) in Rc::make_mut(&mut self.bindings).iter_mut() {
            let value = if name == "#" {
                BindingValue::RuntimeUnknown(None)
            } else if name.parse::<usize>().is_ok() {
                BindingValue::Inherited
            } else {
                continue;
            };
            if !binding.values.contains(&value) {
                Rc::make_mut(&mut binding.values).push(value);
            }
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
        self.assign_binding(name, values);
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
        let origins = if !self.captured {
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
            .map_or(Export::No, |binding| binding.exported);
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
                values: Rc::new(values),
                exported,
                arithmetic,
            },
        );
    }
}
