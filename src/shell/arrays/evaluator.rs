use super::*;

impl Evaluator<'_, '_> {
    pub(in crate::shell) fn array_assignment(
        &mut self,
        name: &str,
        values: &[(Option<RawWord>, RawWord)],
        append: bool,
        declaration: bool,
        scope: &mut Scope,
        depth: usize,
    ) -> Result<(), CheckError> {
        let mut state = if append {
            array(scope, name).unwrap_or_else(|| IndexedArray::new(self.frontend.zsh))
        } else {
            IndexedArray::new(self.frontend.zsh)
        };
        for (index, raw) in values {
            let mut addition: Option<IndexedArray> = None;
            for expanded in self.expand(raw, scope, depth)? {
                let mut candidate = IndexedArray::new(self.frontend.zsh);
                candidate.exact &= !expanded.unknown_splitting;
                let zsh_unsplit = self.frontend.zsh
                    && !expanded.positional
                    && expanded.split.len() > 1
                    && !expanded.word.expands
                    && !expanded.word.runtime_unknown;
                for word in &expanded.split {
                    #[cfg(test)]
                    {
                        self.output.array_words += 1;
                    }
                    candidate.exact &= !word.field_count_unknown;
                    candidate.push(vec![snapshot(word.clone())]);
                }
                if let Some(zsh) = &mut candidate.zsh {
                    zsh.exact &= !expanded.unknown_splitting;
                    if zsh_unsplit {
                        zsh.elements.clear();
                        zsh.push_one(vec![snapshot(expanded.word)]);
                    }
                }
                if let Some(addition) = &mut addition {
                    addition.merge(&candidate);
                } else {
                    addition = Some(candidate);
                }
            }
            if let Some(addition) = addition {
                if index
                    .as_ref()
                    .is_some_and(|index| integer(&index.raw, scope) == Some(0))
                {
                    state.zsh = None;
                }
                if let Some(zsh) = &mut state.zsh
                    && let Some(other) = &addition.zsh
                {
                    zsh.append_view(other, index.as_ref(), scope);
                }
                state.append_view(&addition, index.as_ref(), scope);
            }
        }
        if declaration && scope.in_function() {
            scope.local(name);
        }
        store(scope, name, state);
        Ok(())
    }

    pub(in crate::shell) fn read_array_fields(
        &mut self,
        name: &str,
        input: Option<&[String]>,
        unknown: bool,
        scope: &mut Scope,
    ) {
        let mut state: Option<IndexedArray> = None;
        let contexts = scope.ifs_candidates();
        if scope
            .bindings
            .get("IFS")
            .is_some_and(|binding| binding.values.iter().any(|value| value.known().is_none()))
        {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
        }
        for value in input.into_iter().flatten() {
            for ifs in &contexts {
                let mut candidate = IndexedArray::new(self.frontend.zsh);
                for field in words::fields(value, &[], ifs.as_deref()) {
                    candidate.push(vec![Word::literal(field.into())]);
                }
                if let Some(state) = &mut state {
                    state.merge(&candidate);
                } else {
                    state = Some(candidate);
                }
            }
        }
        let mut state = state.unwrap_or_else(|| IndexedArray::unknown(self.frontend.zsh));
        if unknown {
            state.merge(&IndexedArray::unknown(self.frontend.zsh));
        }
        if let Some(prior) = array(scope, name) {
            if self.frontend.zsh
                || prior
                    .binding_values()
                    .iter()
                    .filter_map(BindingValue::known)
                    .any(|value| {
                        !matches!(
                            crate::shell::arithmetic::armed(value),
                            crate::shell::arithmetic::Arming::Inert
                        )
                    })
            {
                state.merge(&prior);
            }
        } else if let Some(prior) = scope.bindings.get(name)
            && (self.frontend.zsh
                || prior
                    .values
                    .iter()
                    .filter_map(BindingValue::known)
                    .any(|value| {
                        !matches!(
                            crate::shell::arithmetic::armed(value),
                            crate::shell::arithmetic::Arming::Inert
                        )
                    }))
        {
            state.merge(&IndexedArray::scalar(self.frontend.zsh, &prior.values));
        }
        store(scope, name, state);
    }

    pub(in crate::shell) fn read_array_lines(
        &mut self,
        args: &[Word],
        scope: &mut Scope,
    ) -> Result<(), crate::CheckError> {
        let mut trim = false;
        let mut name = "MAPFILE";
        let mut modeled = true;
        for word in args {
            if word == "-t" {
                trim = true;
            } else if crate::shell::statements::identifier(&word.text) {
                name = &word.text;
            } else {
                modeled = false;
            }
        }
        let mut state: Option<IndexedArray> = None;
        let mut unknown = true;
        if modeled {
            let candidates = scope
                .pipeline_input
                .as_ref()
                .map(|input| self.flow.candidates(input))
                .transpose()?;
            unknown = candidates
                .as_ref()
                .is_none_or(|values| values.iter().any(|value| value.unknown));
            for output in candidates
                .as_ref()
                .into_iter()
                .flatten()
                .filter(|value| value.known)
                .map(|value| &value.text)
            {
                let mut candidate = IndexedArray::new(self.frontend.zsh);
                for line in output.split_inclusive('\n') {
                    candidate.push(vec![Word::literal(
                        if trim {
                            line.trim_end_matches('\n')
                        } else {
                            line
                        }
                        .into(),
                    )]);
                }
                if let Some(state) = &mut state {
                    state.merge(&candidate);
                } else {
                    state = Some(candidate);
                }
            }
        }
        if let Some(state) = state {
            let mut state = state;
            if unknown {
                state.merge(&IndexedArray::unknown(self.frontend.zsh));
            }
            if let Some(prior) = array(scope, name).filter(|_| self.frontend.zsh) {
                state.merge(&prior);
            } else if let Some(prior) = scope.bindings.get(name).filter(|_| self.frontend.zsh) {
                state.merge(&IndexedArray::scalar(self.frontend.zsh, &prior.values));
            }
            store(scope, name, state);
        } else {
            self.read_array_fields(name, None, true, scope);
        }
        Ok(())
    }
}
