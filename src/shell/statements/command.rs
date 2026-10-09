use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn statement(
        &mut self,
        statement: &Statement,
        scope: &mut Scope,
        depth: usize,
        source_id: usize,
        nested: bool,
    ) -> Result<Output, CheckError> {
        // Keep compound temporaries off the recursively entered command frame.
        #[cfg(test)]
        {
            self.output.failure_copies += scope.directory.failures.as_deref().map_or(0, Vec::len);
            self.output.cwd_candidates += scope.directory.alternatives.len() + 1;
        }
        let Statement::Command {
            assignments,
            argv,
            redirects,
            pipeline,
        } = statement
        else {
            return self.compound_statement(statement, scope, depth, source_id, nested);
        };
        let mut command_bindings = BTreeMap::new();
        let prefixes = self.command_assignments(
            assignments,
            !argv.is_empty(),
            scope,
            depth,
            &mut command_bindings,
        )?;
        let arguments = self.command_arguments(argv, scope, depth)?;
        let alternatives =
            self.command_alternatives(prefixes, &arguments, scope, (*pipeline, nested));
        let targets = self.expanded_redirects(redirects, scope, depth)?;
        let entry = scope.clone();
        let mut exits = Vec::new();
        let mut outputs = Vec::new();
        for alternative in alternatives {
            let mut branch = entry.clone();
            outputs.push(self.command_alternative(
                alternative.into_words(),
                &mut branch,
                &targets,
                &command_bindings,
                (depth, source_id, nested, *pipeline),
            )?);
            exits.push(branch);
        }
        if let Some(first) = exits.first() {
            scope.directory = first.directory.clone();
            if let Some(failures) = &mut scope.directory.failures {
                for branch in exits.iter().skip(1) {
                    Rc::make_mut(failures).extend(
                        branch
                            .directory
                            .failures
                            .as_deref()
                            .into_iter()
                            .flatten()
                            .cloned(),
                    );
                }
            }
        }
        self.merge_directories(scope, &exits);
        self.merge_bindings(scope, &exits);
        Ok(self.flow.outputs(&outputs, true))
    }

    pub(super) fn expanded_redirects(
        &mut self,
        redirects: &[shell::RawRedirect],
        scope: &mut Scope,
        depth: usize,
    ) -> Result<Vec<crate::record::Redirect>, CheckError> {
        let mut targets = Vec::new();
        for redirect in redirects {
            for expanded in self.expand(&redirect.target, scope, depth)? {
                let words = if scope.repeated_word(&expanded.word) {
                    expanded.split
                } else {
                    vec![expanded.word]
                };
                targets.extend(words.into_iter().map(|word| {
                    crate::record::Redirect::from_word(
                        word,
                        redirect.direction,
                        redirect.fd,
                        redirect.duplicate,
                    )
                }));
            }
        }
        Ok(targets)
    }

    fn command_alternative(
        &mut self,
        mut argv: Vec<crate::record::Word>,
        scope: &mut Scope,
        targets: &[crate::record::Redirect],
        command_bindings: &BTreeMap<String, Vec<BindingValue>>,
        context: (usize, usize, bool, Option<usize>),
    ) -> Result<Output, CheckError> {
        let (depth, source_id, nested, pipeline) = context;
        let flow_end = scope.flow_end;
        let input = scope.pipeline_input.clone();
        let input_fds = scope.input_fds.clone();
        let stdin_id = scope.stdin_id;
        let output_fds = scope.output_fds.clone();
        self.redirect_fds(scope, targets);
        let mut resolved = shell::argv::resolve(
            &mut argv,
            &scope.directory.current.render(),
            self.frontend.host,
        );
        let program = resolved.program;
        if let Some(gap) = resolved.gap.take() {
            self.output.gap(gap);
        }
        let prior = command_bindings
            .keys()
            .map(|name| (name.clone(), scope.bindings.get(name).cloned()))
            .collect::<BTreeMap<_, _>>();
        let return_start = scope.returns.len();
        for (name, values) in command_bindings {
            scope.assign(name.clone(), values.clone());
        }
        let mut executed_output =
            self.command_builtin(&mut argv, &resolved, scope, &prior, depth)?;
        let shell_source = self.observed_shell_source(&mut argv, scope, program);
        let command = Command {
            environment: Vec::new(),
            function: resolved.wrappers.is_empty()
                && program.is_some_and(|i| self.functions.contains_key(&argv[i].text)),
            argv,
            redirects: targets.to_vec(),
            cwd: resolved.cwd,
            program,
            wrappers: resolved.wrappers,
            shell: resolved.shell,
            flags: Vec::new(),
            items: None,
            stdin: if scope.piped {
                Stdin::Inherited
            } else {
                Stdin::None
            },
            pipeline: pipeline.map(|id| (source_id, id)),
            nested,
        };
        let emitted = self.output.script.commands.len();
        let command = self.emit(command, scope);
        let emitted_end = self.output.script.commands.len();
        if let Some(output) = self.command_shell_source(
            &command,
            scope,
            &prior,
            (&resolved.environment, depth),
            shell_source.as_deref(),
        )? {
            executed_output = Some(output);
        }
        if let Some(source) = resolved.source {
            self.isolated_source(&source, scope, depth + 1)?;
        }
        if let Some(output) = self.command_stdin_source(
            &command,
            scope,
            &prior,
            &resolved.environment,
            (emitted, emitted_end, depth),
        )? {
            executed_output = Some(output);
        }
        self.command_xargs_sources(&command, depth)?;
        if let Some(output) = self.command_function(&command, scope, (depth, nested, flow_end))? {
            executed_output = Some(output);
        }
        let output = self.command_completion(
            &command,
            scope,
            &prior,
            (return_start, emitted, emitted_end),
            executed_output,
        );
        scope.output_fds = output_fds;
        if targets
            .iter()
            .any(|target| target.direction != crate::record::Direction::Out)
        {
            scope.restore_inputs(stdin_id, input, input_fds);
        }
        Ok(output)
    }

    fn command_completion(
        &mut self,
        command: &Command,
        scope: &mut Scope,
        prior: &BTreeMap<String, Option<Binding>>,
        emitted: (usize, usize, usize),
        executed_output: Option<Output>,
    ) -> Output {
        let (return_start, emitted, emitted_end) = emitted;
        if !command.function {
            self.track(command, scope);
        }
        if !prior.is_empty() {
            restore_prefix(&mut scope.bindings, prior);
            for state in Rc::make_mut(&mut scope.returns)
                .iter_mut()
                .skip(return_start)
            {
                restore_prefix(&mut state.bindings, prior);
            }
        }
        if let Some(output) = executed_output {
            output
        } else {
            let records = self.output.script.commands[emitted..emitted_end].to_vec();
            let outputs = records
                .iter()
                .map(|record| self.command_output(record, scope))
                .collect::<Vec<_>>();
            self.flow.outputs(&outputs, true)
        }
    }
}
