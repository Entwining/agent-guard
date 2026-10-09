use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub fn new(frontend: Frontend<'a>, output: &'b mut Observation) -> Self {
        Self {
            frontend,
            output,
            functions: Rc::default(),
            running: BTreeSet::new(),
            inspected: 0,
            function_runs: 0,
            unresolved_calls: Vec::new(),
            function_namespaces: Vec::new(),
            namespace: None,
            deadline: None,
            parse_cache: BTreeMap::new(),
            flow: FlowBuilder::default(),
        }
    }
    pub fn finish(&mut self) {
        if let Some(gap) = self.flow.gap() {
            self.output.gap(gap);
        }
        #[cfg(test)]
        {
            self.output.flow_nodes = self.flow.nodes;
            self.output.flow_pairs = self.flow.pairs;
            self.output.flow_visits = self.flow.visits;
            self.output.flow_guard_pairs = self.flow.guard_pairs;
        }
        for (index, defining, namespace) in &self.unresolved_calls {
            let functions = namespace
                .map(|namespace| &self.function_namespaces[namespace])
                .unwrap_or(&self.functions);
            let command = &mut self.output.script.commands[*index];
            if let Some(program) = command.program
                && functions.contains_key(&command.argv[program].text)
                && crate::targets::infer(command, &command.cwd, self.frontend.host)
                    .gaps
                    .iter()
                    .any(|g| matches!(g, CoverageGap::UnknownProgram { .. }))
            {
                if *defining {
                    command.function = true;
                } else {
                    self.output.gap(CoverageGap::UnsupportedShellSyntax);
                }
            }
        }
    }
    pub fn run(
        &mut self,
        body: &[Statement],
        scope: &mut Scope,
        depth: usize,
        source_id: usize,
        nested: bool,
    ) -> Result<Output, CheckError> {
        self.flow.deadline = self.deadline;
        let mut outputs = Vec::new();
        let mut exits = Vec::new();
        if depth > MAX_NESTING {
            self.output.gap(CoverageGap::InspectionBudget);
            return Ok(Output::new());
        }
        for statement in body {
            crate::check_deadline(self.deadline)?;
            #[cfg(test)]
            {
                self.output.statement_visits += 1;
            }
            self.inspected += 1;
            if self.inspected > 512 {
                self.output.gap(CoverageGap::InspectionBudget);
                return Ok(Output::new());
            }
            let before =
                scope.returns.len() + scope.loops.iter().map(|states| states.len()).sum::<usize>();
            outputs.push(self.statement(statement, scope, depth, source_id, nested)?);
            if scope.captured {
                let after = scope
                    .returns
                    .iter()
                    .chain(scope.loops.iter().flat_map(|states| states.iter()))
                    .collect::<Vec<_>>();
                if after.len() > before {
                    let prefix = self.flow.outputs(&std::mem::take(&mut outputs), false);
                    let mut continuing = prefix.clone();
                    for state in after.iter().skip(before) {
                        exits.push(self.flow.filter_output(&prefix, &state.flow_guard, false));
                        continuing = self
                            .flow
                            .filter_output(&continuing, &state.flow_guard, true);
                    }
                    outputs.push(continuing);
                }
            }
        }
        exits.push(self.flow.outputs(&outputs, false));
        Ok(self.flow.outputs(&exits, true))
    }
}
