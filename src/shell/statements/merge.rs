use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn merge_bindings(&mut self, scope: &mut Scope, branches: &[Scope]) {
        if scope.join(branches) {
            self.output.gap(CoverageGap::InspectionBudget);
        }
        if scope.captured {
            for (name, binding) in Rc::make_mut(&mut scope.bindings) {
                if let Some(first) = branches
                    .first()
                    .and_then(|branch| branch.bindings.get(name))
                    && branches
                        .iter()
                        .all(|branch| branch.bindings.get(name) == Some(first))
                {
                    binding.origins = first.origins.clone();
                    continue;
                }
                let mut origins = Origins::new();
                for branch in branches {
                    if let Some(prior) = branch.bindings.get(name) {
                        for value in prior
                            .values
                            .iter()
                            .flat_map(|value| match value {
                                BindingValue::Array(array) => array.binding_values(),
                                value => vec![value.clone()],
                            })
                            .filter_map(|value| value.known().cloned())
                        {
                            let guards = prior
                                .origins
                                .as_ref()
                                .and_then(|origins| origins.get(&value))
                                .cloned()
                                .unwrap_or_else(|| vec![branch.flow_guard.as_ref().clone()]);
                            let destination = origins.entry(value).or_default();
                            for guard in guards {
                                if let Some(guard) = compatible(&guard, &branch.flow_guard)
                                    && !destination.contains(&guard)
                                {
                                    destination.push(guard);
                                }
                            }
                        }
                    }
                }
                if !origins.is_empty() {
                    binding.origins = Some(Rc::new(origins));
                }
            }
        }
        let owners = scope
            .input_fds
            .values()
            .copied()
            .chain([scope.stdin_id])
            .collect::<BTreeSet<_>>();
        for owner in owners {
            let values = branches
                .iter()
                .map(|branch| {
                    branch.input_cursors.get(&owner).cloned().or_else(|| {
                        (branch.stdin_id == owner)
                            .then(|| branch.pipeline_input.clone())
                            .flatten()
                    })
                })
                .collect::<Vec<_>>();
            if values.iter().all(Option::is_none) {
                continue;
            }
            let flow = if values.iter().all(|value| value == &values[0]) {
                values[0].clone().unwrap_or_default()
            } else {
                let parts = values
                    .into_iter()
                    .zip(branches)
                    .map(|(value, branch)| {
                        let value = value.unwrap_or_else(|| self.flow.unknown());
                        let guard = self
                            .flow
                            .bytes(String::new(), branch.flow_guard.as_ref().clone());
                        self.flow.sequence(vec![guard, value])
                    })
                    .collect();
                self.flow.choice(parts)
            };
            Rc::make_mut(&mut scope.input_cursors).insert(owner, flow.clone());
            if owner == scope.stdin_id {
                scope.pipeline_input = Some(flow);
            }
        }
    }

    pub(super) fn merge_directories(&mut self, scope: &mut Scope, branches: &[Scope]) {
        let mut candidates = Vec::new();
        for branch in branches {
            candidates.push(branch.directory.current.clone());
            candidates.extend(branch.directory.alternatives.iter().cloned());
            scope.directory.relative_growth = scope
                .directory
                .relative_growth
                .max(branch.directory.relative_growth);
            scope.directory.gap = scope.directory.gap.take().or(branch.directory.gap.clone());
        }
        scope
            .directory
            .merge(candidates.into_iter(), self.frontend.host.home);
    }
}
