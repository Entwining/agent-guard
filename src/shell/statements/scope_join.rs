use super::*;

impl Scope {
    pub(super) fn join(&mut self, branches: &[Scope]) -> bool {
        self.pipeline_input = if branches.iter().all(|branch| {
            branch.pipeline_input
                == branches
                    .first()
                    .and_then(|branch| branch.pipeline_input.clone())
        }) {
            branches
                .first()
                .and_then(|branch| branch.pipeline_input.clone())
        } else {
            None
        };
        self.flow_end = !branches.is_empty() && branches.iter().all(|branch| branch.flow_end);
        let mut bounded = false;
        if let Some(first) = branches.first().filter(|first| {
            first.bindings.values().all(Binding::join_is_identity)
                && branches
                    .iter()
                    .all(|branch| Rc::ptr_eq(&first.bindings, &branch.bindings))
        }) {
            self.bindings = first.bindings.clone();
        } else {
            let keys = branches
                .iter()
                .flat_map(|s| s.bindings.keys())
                .cloned()
                .collect::<BTreeSet<_>>();
            let mut bindings = BTreeMap::new();
            for name in keys {
                let (value, bound) = join_values(branches.iter().map(|s| s.bindings.get(&name)));
                bounded |= bound;
                if let Some(value) = value {
                    bindings.insert(name, value);
                }
            }
            self.bindings = Rc::new(bindings);
        }
        bounded |= self.join_frames(branches);
        for branch in branches {
            self.relative_glob_moves = self.relative_glob_moves.max(branch.relative_glob_moves);
            for state in branch.returns.iter() {
                if !self.returns.contains(state) {
                    if self.returns.len() == 512 {
                        bounded = true;
                    } else {
                        Rc::make_mut(&mut self.returns).push(state.clone());
                    }
                }
            }
            for (index, inner) in branch.loops.iter().enumerate().take(self.loops.len()) {
                for state in inner.iter() {
                    if !self.loops[index].contains(state) {
                        if self.loops[index].len() == 512 {
                            bounded = true;
                        } else {
                            Rc::make_mut(&mut Rc::make_mut(&mut self.loops)[index])
                                .push(state.clone());
                        }
                    }
                }
            }
        }
        bounded
    }
    fn join_frames(&mut self, branches: &[Scope]) -> bool {
        let mut bounded = false;
        for index in 0..self.frames.len() {
            if let Some(first) = branches.first().filter(|first| {
                first.frames[index]
                    .values()
                    .flatten()
                    .all(Binding::join_is_identity)
                    && branches
                        .iter()
                        .all(|branch| Rc::ptr_eq(&first.frames[index], &branch.frames[index]))
            }) {
                if !Rc::ptr_eq(&self.frames[index], &first.frames[index]) {
                    Rc::make_mut(&mut self.frames)[index] = first.frames[index].clone();
                }
                continue;
            }
            let frame = Rc::make_mut(&mut Rc::make_mut(&mut self.frames)[index]);
            let names = branches
                .iter()
                .flat_map(|s| s.frames[index].keys())
                .cloned()
                .collect::<BTreeSet<_>>();
            for name in names {
                let (value, bound) = join_values(branches.iter().map(|s| {
                    s.frames[index]
                        .get(&name)
                        .map_or_else(|| s.bindings.get(&name), Option::as_ref)
                }));
                bounded |= bound;
                frame.insert(name, value);
            }
        }
        bounded
    }
}

pub(super) fn join_values<'a>(
    values: impl Iterator<Item = Option<&'a Binding>>,
) -> (Option<Binding>, bool) {
    let values: Vec<_> = values.collect();
    if let Some(first) = values.first().copied().flatten().filter(|first| {
        first.join_is_identity()
            && values.iter().all(|binding| {
                binding.is_some_and(|binding| {
                    Rc::ptr_eq(&first.values, &binding.values)
                        && first.exported == binding.exported
                        && first.arithmetic == binding.arithmetic
                })
            })
    }) {
        return (Some(first.clone()), false);
    }
    let mut joined = Vec::new();
    let mut present = false;
    let mut bounded = false;
    let mut exported = false;
    let mut arithmetic = false;
    for binding in values {
        present |= binding.is_some();
        exported |= binding.is_some_and(|binding| binding.exported);
        arithmetic |= binding.is_some_and(|binding| binding.arithmetic);
        let values = binding.map_or_else(
            || vec![BindingValue::RuntimeUnknown(Some(String::new()))],
            |b| b.values.as_ref().clone(),
        );
        for value in values {
            bounded |= join_candidate(&mut joined, value);
        }
    }
    let mut empty = Vec::new();
    for value in &joined {
        if let BindingValue::Known(text) = value
            && joined.iter().any(|other| {
                matches!(other, BindingValue::RepeatedFields(repetition)
                if text == &format!("{}{}", repetition.prefix, repetition.suffix))
            })
        {
            empty.push(text.clone());
        }
    }
    for value in &mut joined {
        if let BindingValue::RepeatedFields(repetition) = value
            && empty.contains(&format!("{}{}", repetition.prefix, repetition.suffix))
        {
            repetition.may_be_empty = true;
        }
    }
    joined.retain(|value| !matches!(value, BindingValue::Known(text) if empty.contains(text)));
    (
        present.then_some(Binding {
            #[cfg(test)]
            copies: EntryCopies::default(),
            origins: None,
            values: Rc::new(joined),
            exported,
            arithmetic,
        }),
        bounded,
    )
}

fn join_candidate(joined: &mut Vec<BindingValue>, value: BindingValue) -> bool {
    let mut bounded = false;
    if let BindingValue::Array(array) = &value
        && let Some(BindingValue::Array(existing)) = joined
            .iter_mut()
            .find(|value| matches!(value, BindingValue::Array(_)))
    {
        existing.merge(array);
        return bounded;
    }
    if let BindingValue::RepeatedFields(repetition) = &value
        && let Some(BindingValue::RepeatedFields(existing)) = joined.iter_mut().find(|v| {
            matches!(
                v,
                BindingValue::RepeatedFields(other)
                    if other.prefix == repetition.prefix && other.suffix == repetition.suffix
            )
        })
    {
        existing.may_be_empty |= repetition.may_be_empty;
        for alternative in &repetition.alternatives {
            if !existing.alternatives.contains(alternative) {
                if existing.alternatives.len() == 512 {
                    bounded = true;
                } else {
                    existing.alternatives.push(alternative.clone());
                }
            }
        }
        return bounded;
    }
    if !joined.contains(&value) {
        if joined.len() == 512 {
            bounded = true;
        } else {
            joined.push(value);
        }
    }
    bounded
}
