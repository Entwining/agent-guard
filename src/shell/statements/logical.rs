use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn logical_list(
        &mut self,
        statement: &Statement,
        scope: &mut Scope,
        depth: usize,
        source_id: usize,
        nested: bool,
    ) -> Result<Output, CheckError> {
        // The parser's left-associated list does not add syntactic nesting.
        // Preserve each branch continuation without recursive command frames.
        let mut frames = Vec::new();
        let (current, continuations) = cwd::logical_continuations(
            statement,
            #[cfg(test)]
            &mut self.output.logical_guard_visits,
        );
        for continuation in continuations {
            let cwd::LogicalContinuation {
                operator,
                right,
                qualified,
                definition_on_success,
            } = continuation;
            let before = scope.isolated();
            let outer_failures = if qualified {
                let outer = scope.directory.failures.take();
                scope.directory.failures = Some(Rc::default());
                outer
            } else {
                None
            };
            frames.push((
                operator,
                right,
                before,
                qualified,
                outer_failures,
                definition_on_success,
            ));
        }
        let mut output = self.statement(current, scope, depth, source_id, nested)?;
        while let Some((
            operator,
            right,
            before,
            qualified,
            outer_failures,
            definition_on_success,
        )) = frames.pop()
        {
            let choice = self.flow.choice_id();
            let mut left_exit = scope.clone();
            Rc::make_mut(&mut left_exit.flow_guard).insert(choice, 0);
            let mut skipped = before.clone();
            Rc::make_mut(&mut skipped.flow_guard).insert(choice, 0);
            let mut after = scope.branch();
            Rc::make_mut(&mut after.flow_guard).insert(choice, 1);
            after.conditional_append = true;
            // Directory-command success does not require a statically known
            // destination. Preserve an enclosing if/case uncertainty.
            if definition_on_success {
                after.conditional_definition = scope.conditional_definition;
            }
            if qualified && !matches!(right, Statement::Command { .. }) {
                after.directory.failures = None;
            }
            let right_output = self.statement(right, &mut after, depth, source_id, nested)?;
            let skip_output = self.guarded_output(&left_exit, &Output::new());
            let right_output = self.guarded_output(&after, &right_output);
            let (skip_output, right_output) = if left_exit.same_flow_state(&after) {
                left_exit.forget_choice(choice);
                after.forget_choice(choice);
                skipped.forget_choice(choice);
                (
                    self.flow.forget_output(skip_output, choice)?,
                    self.flow.forget_output(right_output, choice)?,
                )
            } else {
                (skip_output, right_output)
            };
            let optional = self.flow.outputs(&[skip_output, right_output], true);
            output = self.flow.outputs(&[output, optional], false);
            if matches!(operator, Operator::And) {
                self.logical_directory_exit(scope, &left_exit, &after, qualified, outer_failures);
                self.merge_bindings(scope, &[skipped, left_exit, after]);
            } else {
                self.merge_bindings(scope, &[left_exit, after.clone()]);
                self.merge_directories(scope, &[after]);
            }
        }
        Ok(output)
    }

    fn logical_directory_exit(
        &mut self,
        scope: &mut Scope,
        left_exit: &Scope,
        after: &Scope,
        qualified: bool,
        outer_failures: Option<Rc<Vec<cwd::CwdPath>>>,
    ) {
        let mut failures = left_exit.directory.failures.clone().unwrap_or_default();
        cwd::extend_unique(
            Rc::make_mut(&mut failures),
            after
                .directory
                .failures
                .as_deref()
                .into_iter()
                .flatten()
                .cloned(),
        );
        scope.directory = after.directory.clone();
        if qualified {
            scope.directory.failures = outer_failures.map(|mut outer| {
                cwd::extend_unique(Rc::make_mut(&mut outer), failures.iter().cloned());
                outer
            });
            if scope.directory.failures.is_none() {
                scope.directory.merge(
                    Rc::unwrap_or_clone(failures).into_iter(),
                    self.frontend.host.home,
                );
            }
        }
    }
}
