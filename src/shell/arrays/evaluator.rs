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

    pub(in crate::shell) fn read_unknown_array(&self, name: &str, scope: &mut Scope) {
        store(scope, name, IndexedArray::unknown(self.frontend.zsh));
    }

    pub(in crate::shell) fn read_array_lines(&self, args: &[Word], scope: &mut Scope) {
        let name = args
            .iter()
            .rfind(|word| crate::shell::statements::identifier(&word.text))
            .map_or("MAPFILE", |word| word.text.as_str());
        self.read_unknown_array(name, scope);
    }
}
