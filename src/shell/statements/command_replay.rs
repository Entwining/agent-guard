use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    /// The code argument of a shell whose invocation is proven: an exact
    /// program, every wrapper word placed, and every option fixed.
    fn proven_shell_code(
        argv: &[crate::record::Word],
        resolved: &shell::argv::Resolution,
    ) -> Option<usize> {
        let index = resolved
            .program
            .filter(|index| resolved.unproven.is_none() && argv[*index].exact().is_some())?;
        let invocation =
            crate::targets::shell_invocation(argv[index].rsplit('/').next()?, &argv[index + 1..])?;
        match invocation.source {
            crate::targets::ShellSource::Code(Some(at)) if invocation.posix => Some(index + 1 + at),
            _ => None,
        }
    }

    pub(super) fn observed_shell_source(
        &self,
        argv: &mut [crate::record::Word],
        scope: &Scope,
        resolved: &shell::argv::Resolution,
    ) -> Option<Box<(String, String)>> {
        let code_argument = Self::proven_shell_code(argv, resolved);
        let source = code_argument.and_then(|at| {
            scope
                .repetition_source(&argv[at])
                .map(|source| (at, source))
        });
        if let Some(at) = code_argument.filter(|at| {
            source.is_some() || argv[*at].exact().is_some() && !argv[*at].cardinality_unknown
        }) {
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
        let Some(index) = command.program.filter(|_| !command.function) else {
            return Ok(None);
        };
        let args = &command.argv[index + 1..];
        let Some(invocation) = command.argv[index]
            .rsplit('/')
            .next()
            .and_then(|program| crate::targets::shell_invocation(program, args))
            .filter(|invocation| invocation.posix)
        else {
            return Ok(None);
        };
        match invocation.source {
            crate::targets::ShellSource::Code(Some(at)) => {
                let mut child = self.shell_child(command, scope, prior, environment);
                let code = if let Some((name, source)) = shell_source {
                    if let Some(binding) = scope.bindings.get(name) {
                        Rc::make_mut(&mut child.bindings).insert(name.clone(), binding.clone());
                    }
                    source.as_str()
                } else {
                    &args[at].text
                };
                let output = self.shell_source(code, &mut child, environment, depth + 1)?;
                // Replaying a representative only adds observations; its
                // output is not the command's.
                Ok(
                    (shell_source.is_some() || args[at].role == Role::ObservedShellCode)
                        .then_some(output),
                )
            }
            // Each fixed argument that some reading makes the code is
            // inspected in the child the shell would start; neither its
            // state nor its output is the command's.
            crate::targets::ShellSource::Uncertain(candidates) => {
                for code in candidates.iter().map(|at| &args[*at]) {
                    if code.exact().is_some() && !code.cardinality_unknown {
                        let mut child = self.shell_child(command, scope, prior, environment);
                        self.shell_source(&code.text, &mut child, environment, depth + 1)?;
                    }
                }
                Ok(None)
            }
            _ => Ok(None),
        }
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
            // Only a proven shell reading its standard input as code makes
            // that code's output the command's.
            let proven = command.unproven.is_none()
                && command.program.is_some_and(|index| {
                    command.argv[index].exact().is_some_and(|program| {
                        crate::targets::shell_invocation(
                            program.rsplit('/').next().unwrap_or(program),
                            &command.argv[index + 1..],
                        )
                        .is_some_and(|invocation| {
                            invocation.source == crate::targets::ShellSource::Stdin
                        })
                    })
                });
            return Ok(proven.then(|| self.flow.outputs(&results, true)));
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
                            &mut Scope::for_host(self.frontend.host, &input.cwd),
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
                            &mut Scope::for_host(self.frontend.host, &input.cwd),
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
