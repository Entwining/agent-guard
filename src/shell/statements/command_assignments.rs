use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    fn append_binding(
        &mut self,
        assignment_scope: &Scope,
        name: &str,
        binding: &[BindingValue],
    ) -> Vec<BindingValue> {
        let prior = assignment_scope.bindings.get(name).map_or_else(
            || vec![BindingValue::Known(String::new())],
            |binding| binding.values.as_ref().clone(),
        );
        let mut combined = Vec::new();
        for left in &prior {
            for right in binding {
                if combined.len() == 512 {
                    self.output.gap(CoverageGap::InspectionBudget);
                    break;
                }
                let value = match (left, right) {
                    (BindingValue::Known(left), BindingValue::Known(right)) => {
                        BindingValue::Known(format!("{left}{right}"))
                    }
                    (BindingValue::Undetermined, _) | (_, BindingValue::Undetermined) => {
                        BindingValue::Undetermined
                    }
                    _ => BindingValue::RuntimeDerived(format!(
                        "{}{}",
                        left.lexical().map_or("", String::as_str),
                        right.lexical().map_or("", String::as_str),
                    )),
                };
                if !combined.contains(&value) {
                    combined.push(value);
                }
            }
        }
        combined
    }

    pub(super) fn command_assignments(
        &mut self,
        assignments: &[(String, RawWord)],
        has_argv: bool,
        scope: &mut Scope,
        depth: usize,
        command_bindings: &mut BTreeMap<String, Vec<BindingValue>>,
    ) -> Result<Vec<crate::record::Word>, CheckError> {
        let mut prefixes = Vec::new();
        let mut assignment_scope = scope.clone();
        for (name, raw) in assignments {
            let observe_bindings = assignment_scope
                .bindings
                .get(name.strip_suffix('+').unwrap_or(name))
                .is_some_and(|binding| binding.arithmetic);
            // Copying stores the armed value; only an arithmetic attribute
            // consumes it here. A later assignment replaces the stored value.
            let accumulated = self.literal_accumulation(name, raw, &mut assignment_scope, depth)?;
            let values = if accumulated.is_some() {
                Vec::new()
            } else {
                shell::expand_scoped(raw, &mut assignment_scope, self, depth, observe_bindings)?
            };
            for value in &values {
                self.armed_references(&value.word.text, &mut assignment_scope, depth)?;
            }
            let origins = assignment_scope.value_origins(&values, &mut self.flow);
            let mut binding = accumulated.unwrap_or_else(|| {
                values
                    .iter()
                    .map(|v| assignment_scope.expanded_binding(&v.word, &v.word.text))
                    .collect::<Vec<_>>()
            });
            let (name, append) = name
                .strip_suffix('+')
                .map_or((name.as_str(), false), |name| (name, true));
            if append {
                binding = self.append_binding(&assignment_scope, name, &binding);
            }
            assignment_scope.assign(name.to_owned(), binding.clone());
            if !append && !origins.is_empty() {
                assignment_scope.set_origins(name, origins.clone());
            }
            if !has_argv {
                scope.assign(name.to_owned(), binding.clone());
                if !append && !origins.is_empty() {
                    scope.set_origins(name, origins);
                }
            } else {
                command_bindings.insert(name.to_owned(), binding);
            }
            let mut word = values
                .first()
                .map(|v| v.word.clone())
                .unwrap_or_else(|| crate::record::Word::literal(String::new()));
            word.text = format!("{name}={}", word.text);
            for range in word.cwd_ranges.iter_mut().chain(&mut word.quoted_ranges) {
                range.start += name.len() + 1;
                range.end += name.len() + 1;
            }
            word.value = word.text.clone();
            word.role = if !has_argv {
                Role::Precommand
            } else {
                Role::Assign
            };
            prefixes.push(word);
        }
        scope.inherit_input_progress(&assignment_scope);
        Ok(prefixes)
    }
}
