use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn loop_statement(
        &mut self,
        variable: Option<&str>,
        header: &[RawWord],
        body: &[Statement],
        empty: bool,
        scope: &mut Scope,
        context: (usize, usize, bool),
    ) -> Result<Output, CheckError> {
        let (depth, _, nested) = context;

        let before = scope.clone();
        let mut inner = scope.branch();
        inner.directory.failures = None;
        let header_state = self.loop_header(variable, header, scope, (depth, nested))?;
        inner.bounded_loop =
            variable.is_some() && header_state.finite && header_state.count.is_some();
        inner.conditional_append = false;
        let ordered_capture = scope.captured
            && header_state
                .ordered_values
                .iter()
                .any(|(_, uncertain)| !uncertain);
        if !empty
            && (!ordered_capture || header_state.literal)
            && !header_state.values.is_empty()
            && let Some(variable) = variable
            && self.loop_body_is_invariant(body, scope, variable, ordered_capture)?
        {
            return self.invariant_loop(
                variable,
                body,
                scope,
                (before, inner),
                header_state,
                context,
            );
        }
        if (header_state.literal || ordered_capture)
            && header_state.ordered_values.len() <= 512
            && let Some(variable) = variable
        {
            return self.ordered_loop(
                variable,
                body,
                scope,
                (before, inner),
                header_state.ordered_values,
                context,
            );
        }
        self.converging_loop(
            (variable, empty),
            body,
            scope,
            (before, inner),
            header_state,
            context,
        )
    }

    fn invariant_loop(
        &mut self,
        variable: &str,
        body: &[Statement],
        scope: &mut Scope,
        states: (Scope, Scope),
        header: LoopHeader,
        context: (usize, usize, bool),
    ) -> Result<Output, CheckError> {
        let (depth, source_id, nested) = context;
        let (before, mut inner) = states;
        let LoopHeader {
            values,
            literal,
            literal_values,
            count,
            ..
        } = header;
        let flow_end = scope.flow_end;
        let mut outputs = Vec::new();
        let mut stopped = Vec::new();
        let root = !scope.summarizing_loop;
        inner.summarizing_loop = true;
        inner.assign(variable.to_owned(), values);
        let mut completed = 0;
        loop {
            let prior = inner.clone();
            outputs.push(self.run(body, &mut inner, depth + 1, source_id, nested)?);
            completed += 1;
            if !root
                || inner.same_values_and_input(&prior)
                || count.is_some_and(|count| completed >= count)
            {
                break;
            }
            if self.inspected >= 512 || completed >= 512 || self.output.script.commands.len() > 512
            {
                self.output.gap(CoverageGap::InspectionBudget);
                break;
            }
        }
        if literal
            && scope.captured
            && completed < literal_values.len()
            && let Some(output) = outputs.last().cloned()
        {
            outputs.extend(std::iter::repeat_n(
                output,
                literal_values.len() - completed,
            ));
        }
        inner.summarizing_loop = scope.summarizing_loop;
        inner.bounded_loop = scope.bounded_loop;
        inner.conditional_append = scope.conditional_append;
        if literal {
            if let Some(last) = literal_values.last() {
                inner.assign(variable.to_owned(), vec![BindingValue::Known(last.clone())]);
            }
            inner.conditional_definition = scope.conditional_definition;
            *scope = inner;
        } else {
            self.merge_directories(scope, &[before.clone(), inner.clone()]);
            self.merge_bindings(scope, &[before, inner]);
        }
        scope.flow_end = flow_end;
        let active = self.flow.outputs(&outputs, false);
        stopped.push(active);
        Ok(self.flow.outputs(&stopped, true))
    }

    fn ordered_loop(
        &mut self,
        variable: &str,
        body: &[Statement],
        scope: &mut Scope,
        states: (Scope, Scope),
        ordered_values: Vec<(BindingValue, bool)>,
        context: (usize, usize, bool),
    ) -> Result<Output, CheckError> {
        let (depth, source_id, nested) = context;
        let (before, mut inner) = states;
        let flow_end = scope.flow_end;
        let mut outputs = Vec::new();
        let mut stopped = Vec::new();
        Rc::make_mut(&mut inner.loops).push(Rc::default());
        for (value, uncertain) in ordered_values {
            if inner.directory.relative_growth > before.directory.relative_growth
                && inner.directory.current != before.directory.current
            {
                inner.directory.widen_loop(&before.directory);
            }
            let before_member = inner.clone();
            inner.assign(variable.to_owned(), vec![value]);
            let output_start = outputs.len();
            self.loop_iteration(
                body,
                &mut inner,
                &mut outputs,
                &mut stopped,
                (depth, source_id, nested),
            )?;
            if uncertain {
                let output = self.flow.outputs(&outputs.split_off(output_start), false);
                let output = output
                    .into_iter()
                    .map(|(channel, value)| {
                        let before = self.flow.unknown();
                        let after = self.flow.unknown();
                        (channel, self.flow.sequence(vec![before, value, after]))
                    })
                    .collect();
                outputs.push(self.flow.outputs(&[Output::new(), output], true));
                let after_member = inner.clone();
                self.merge_bindings(&mut inner, &[before_member.clone(), after_member.clone()]);
                self.merge_directories(&mut inner, &[before_member, after_member]);
            }
        }
        let early = Rc::make_mut(&mut inner.loops).pop().ok_or(CheckError {
            kind: crate::CheckErrorKind::GuardFault,
        })?;
        let mut branches = Rc::unwrap_or_clone(early)
            .into_iter()
            .map(|state| inner.with_state(state))
            .collect::<Vec<_>>();
        branches.push(inner.clone());
        scope.directory = inner.directory.clone();
        self.merge_directories(scope, &branches);
        self.merge_bindings(scope, &branches);
        scope.flow_end = flow_end;
        let active = self.flow.outputs(&outputs, false);
        stopped.push(active);
        Ok(self.flow.outputs(&stopped, true))
    }

    fn converging_loop(
        &mut self,
        specification: (Option<&str>, bool),
        body: &[Statement],
        scope: &mut Scope,
        states: (Scope, Scope),
        header_state: LoopHeader,
        context: (usize, usize, bool),
    ) -> Result<Output, CheckError> {
        let (depth, source_id, nested) = context;
        let (variable, empty) = specification;
        let mut outputs = Vec::new();
        let mut stopped = Vec::new();
        let flow_end = scope.flow_end;
        let (before, mut inner) = states;
        let LoopHeader { values, count, .. } = header_state;
        let iterations = if empty {
            Some(0)
        } else if variable.is_some() && header_state.has_words {
            count
        } else {
            None
        };
        if let Some(variable) = variable {
            inner.assign(
                variable.to_owned(),
                if values.is_empty() {
                    vec![BindingValue::RuntimeUnknown(None)]
                } else {
                    values
                },
            );
        }
        Rc::make_mut(&mut inner.loops).push(Rc::default());
        self.loop_iteration(
            body,
            &mut inner,
            &mut outputs,
            &mut stopped,
            (depth, source_id, nested),
        )?;
        let carried_glob_cwd =
            iterations.is_none() && inner.relative_glob_moves > before.relative_glob_moves;
        if carried_glob_cwd {
            // Relative matches depend on the header cwd. Repeating a body
            // that carries cwd cannot treat the pattern as a fixed pathname.
            inner.directory = before.directory.clone();
            inner.directory.gap = Some(CoverageGap::IdentityBound);
            self.output.gap(CoverageGap::IdentityBound);
        }
        let mut branches = vec![before, inner.clone()];
        let mut completed = 1;
        let mut cwd_widened = false;
        let array_fixed = iterations.is_none() && inner.fixed_repeated_array(&branches[0]);
        while !carried_glob_cwd && !array_fixed && iterations.is_none_or(|n| completed < n) {
            if inner.directory.relative_growth > branches[0].directory.relative_growth
                && (inner.directory.current != branches[0].directory.current
                    || iterations.is_none()
                        && inner.directory.alternatives != branches[0].directory.alternatives)
            {
                if iterations.is_none() {
                    if !cwd_widened {
                        inner.directory.widen_unknown_loop(&branches[0].directory);
                        cwd_widened = true;
                    }
                } else {
                    inner.directory.widen_loop(&branches[0].directory);
                }
            }
            let prior = inner.clone();
            self.loop_iteration(
                body,
                &mut inner,
                &mut outputs,
                &mut stopped,
                (depth, source_id, nested),
            )?;
            if iterations.is_none() {
                widen_runtime_repetition(&prior, &mut inner);
            }
            completed += 1;
            branches.push(inner.clone());
            if inner.same_values_and_input(&prior) {
                break;
            }
            if self.inspected >= 512 || completed >= 512 || self.output.script.commands.len() > 512
            {
                self.output.gap(CoverageGap::InspectionBudget);
                break;
            }
        }
        let early = Rc::make_mut(&mut inner.loops).pop().ok_or(CheckError {
            kind: crate::CheckErrorKind::GuardFault,
        })?;
        branches.extend(
            Rc::unwrap_or_clone(early)
                .into_iter()
                .map(|state| inner.with_state(state)),
        );
        if empty {
            branches.truncate(1);
        }
        self.merge_directories(scope, &branches);
        self.merge_bindings(scope, &branches);
        scope.flow_end = flow_end;
        let active = self.flow.outputs(&outputs, false);
        stopped.push(active);
        Ok(self.flow.outputs(&stopped, true))
    }
}

impl Scope {
    fn fixed_repeated_array(&self, before: &Scope) -> bool {
        self.bindings == before.bindings
            && self.directory.current == before.directory.current
            && self.directory.alternatives == before.directory.alternatives
            && self.bindings.values().any(|binding| {
                binding
                    .values
                    .iter()
                    .any(|value| matches!(value, BindingValue::Array(array) if array.repeated()))
            })
    }
}
