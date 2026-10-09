use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn observed_shell_source(
        &self,
        argv: &mut [crate::record::Word],
        scope: &Scope,
        program: Option<usize>,
    ) -> Option<Box<(String, String)>> {
        let code_argument = program
            .filter(|index| {
                matches!(
                    argv[*index].rsplit('/').next(),
                    Some("sh" | "bash" | "zsh" | "dash" | "ksh")
                )
            })
            .and_then(|index| {
                (index + 1..argv.len().saturating_sub(1))
                    .find(|at| shell::argv::shell_code_flag(&argv[*at]))
            })
            .map(|at| at + 1);
        let source = code_argument.and_then(|at| {
            scope
                .repetition_source(&argv[at])
                .map(|source| (at, source))
        });
        if let Some(source) = &source {
            argv[source.0].cardinality_unknown = false;
            argv[source.0].field_count_unknown = false;
        }
        if let Some(at) =
            code_argument.filter(|at| !argv[*at].expands && !argv[*at].cardinality_unknown)
        {
            // The child scope below observes this source with effective
            // exports and wrapper overrides; empty-environment replay loses them.
            argv[at].role = Role::ObservedShellCode;
        }
        source.map(|(_, source)| source)
    }

    pub(super) fn command_shell_source(
        &mut self,
        command: &Command,
        scope: &mut Scope,
        prior: &BTreeMap<String, Option<Binding>>,
        context: (&[shell::argv::EnvironmentChange], usize),
        shell_source: Option<&(String, String)>,
    ) -> Result<Option<Output>, CheckError> {
        let (environment, depth) = context;
        if let Some(index) = command.program.filter(|index| {
            !command.function
                && matches!(
                    command.argv[*index].rsplit('/').next(),
                    Some("sh" | "bash" | "zsh" | "dash" | "ksh")
                )
        }) && let Some(pair) = command.argv[index + 1..]
            .windows(2)
            .find(|pair| shell::argv::shell_code_flag(&pair[0]))
        {
            let mut child = self.shell_child(command, scope, prior, environment);
            let code = if let Some((name, source)) = shell_source {
                if let Some(binding) = scope.bindings.get(name) {
                    Rc::make_mut(&mut child.bindings).insert(name.clone(), binding.clone());
                }
                source.as_str()
            } else {
                &pair[1].text
            };
            let output = self.shell_source(code, &mut child, environment, depth + 1)?;
            return Ok(Some(output));
        }
        Ok(None)
    }

    pub(super) fn command_stdin_source(
        &mut self,
        command: &Command,
        scope: &Scope,
        prior: &BTreeMap<String, Option<Binding>>,
        environment: &[shell::argv::EnvironmentChange],
        context: (usize, usize, usize),
    ) -> Result<Option<Output>, CheckError> {
        let (emitted, emitted_end, depth) = context;
        if !command.function
            && shell::argv::stdin_kind(command) == Stdin::Shell
            && let Some(input) = &scope.pipeline_input
        {
            let candidates = self.flow.candidates(input)?;
            if candidates.iter().all(|candidate| !candidate.unknown) {
                for record in &mut self.output.script.commands[emitted..emitted_end] {
                    record.stdin = Stdin::Shell;
                }
            }
            let mut results = Vec::new();
            let records = self.output.script.commands[emitted..emitted_end].to_vec();
            for record in records {
                for candidate in &candidates {
                    if candidate.known {
                        let mut sources = vec![candidate.text.clone()];
                        if candidate.unknown {
                            let mut previous = 0;
                            for cut in candidate.cuts.iter().copied().chain([candidate.text.len()])
                            {
                                let text = &candidate.text[previous..cut];
                                if !text.is_empty() && !sources.iter().any(|source| source == text)
                                {
                                    sources.push(text.to_owned());
                                }
                                previous = cut;
                            }
                        }
                        for source in sources {
                            if candidate.unknown {
                                let parsed = self.parsed_source(&source)?;
                                if parsed.original.is_none()
                                    || parsed.records.is_none()
                                    || parsed
                                        .detection
                                        .as_ref()
                                        .is_none_or(|detection| detection.divergent)
                                {
                                    continue;
                                }
                            }
                            let mut child = self.shell_child(&record, scope, prior, environment);
                            child.pipeline_input = None;
                            child.stdin_id = self.flow.channel();
                            child.flow_guard = Rc::new(candidate.guard.clone());
                            results.push(self.shell_source(
                                &source,
                                &mut child,
                                environment,
                                depth + 1,
                            )?);
                        }
                    }
                }
            }
            return Ok(Some(self.flow.outputs(&results, true)));
        }
        Ok(None)
    }

    pub(super) fn command_xargs_sources(
        &mut self,
        command: &Command,
        depth: usize,
    ) -> Result<(), CheckError> {
        if command.wrappers.iter().any(|w| w == "xargs") && command.program.is_some() {
            let streams = command
                .redirects
                .iter()
                .filter(|redirect| redirect.direction == crate::record::Direction::In)
                .filter_map(|redirect| redirect.stream.as_deref())
                .chain(crate::targets::list_file_sources(command));
            for stream in streams {
                for output in &stream.known {
                    for input in shell::pipeline::xargs_here_input(command, output) {
                        self.source(
                            &input.source,
                            &mut Scope::new(self.frontend.host.home, &input.cwd),
                            depth + 1,
                        )?;
                    }
                }
            }
            for redirect in &command.redirects {
                if matches!(
                    redirect.direction,
                    crate::record::Direction::Heredoc | crate::record::Direction::Herestring
                ) {
                    for input in shell::pipeline::xargs_here_input(command, &redirect.target) {
                        self.source(
                            &input.source,
                            &mut Scope::new(self.frontend.host.home, &input.cwd),
                            depth + 1,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn command_function(
        &mut self,
        command: &Command,
        scope: &mut Scope,
        context: (usize, bool, bool),
    ) -> Result<Option<Output>, CheckError> {
        let (depth, nested, flow_end) = context;
        if let Some(name) = command
            .program
            .and_then(|i| command.argv.get(i))
            .map(|w| w.text.clone())
            && command.function
            && let Some(function) = self.functions.get(&name).cloned()
        {
            if matches!(name.as_str(), "local" | "declare" | "typeset" | "export") {
                self.output.gap(CoverageGap::UnsupportedShellSyntax);
            }
            if self.running.contains(&name) || self.function_runs == 256 {
                self.output.gap(CoverageGap::InspectionBudget);
            } else {
                self.function_runs += 1;
                self.running.insert(name.clone());
                let caller_returns = std::mem::take(&mut scope.returns);
                let caller_loops = std::mem::take(&mut scope.loops);
                let caller_failures = scope.directory.failures.take();
                scope.enter_function();
                self.positionals(&command.argv[command.program.unwrap_or(0) + 1..], scope);
                let output =
                    self.run(&function.body, scope, depth + 1, function.source_id, nested)?;
                let returns = std::mem::take(&mut scope.returns);
                let mut candidates = vec![scope.clone()];
                candidates.extend(
                    Rc::unwrap_or_clone(returns)
                        .into_iter()
                        .map(|state| scope.with_state(state)),
                );
                for candidate in &mut candidates {
                    candidate.leave_function();
                }
                scope.leave_function();
                self.merge_bindings(scope, &candidates);
                scope.returns = caller_returns;
                scope.loops = caller_loops;
                scope.directory.failures = caller_failures;
                scope.flow_end = flow_end;
                self.running.remove(&name);
                return Ok(Some(output));
            }
        }
        Ok(None)
    }
}
