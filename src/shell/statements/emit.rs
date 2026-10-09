use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn word_use(
        &mut self,
        raw: &RawWord,
        scope: &mut Scope,
        depth: usize,
        nested: bool,
    ) -> Result<(), CheckError> {
        for expanded in self.expand(raw, scope, depth)? {
            self.emit(
                Command {
                    function: false,
                    environment: Vec::new(),
                    argv: vec![expanded.word],
                    redirects: Vec::new(),
                    cwd: scope.directory.current.render(),
                    program: None,
                    wrappers: Vec::new(),
                    shell: true,
                    flags: Vec::new(),
                    items: None,
                    stdin: Stdin::None,
                    pipeline: None,
                    nested,
                },
                scope,
            );
        }
        Ok(())
    }
    pub(super) fn emit(&mut self, mut command: Command, scope: &Scope) -> Command {
        self.command_coverage(&command, scope);
        let data = command
            .redirects
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                matches!(
                    r.direction,
                    crate::record::Direction::Heredoc | crate::record::Direction::Herestring
                )
                .then_some(i)
            })
            .collect::<Vec<_>>();
        if !data.is_empty() {
            command.stdin = match shell::argv::stdin_kind(&command) {
                Stdin::None => Stdin::Data(data),
                kind => kind,
            };
        }
        if command.program.is_some() && !command.function {
            self.unresolved_calls.push((
                self.output.script.commands.len(),
                scope.defining,
                self.namespace,
            ));
        }
        self.output.script.commands.push(command.clone());
        let effects = crate::targets::infer(&command, &command.cwd, self.frontend.host);
        let cwd_dependent = command.argv.iter().any(|word| word.pwd)
            || !effects.code.is_empty()
            || !effects.inline.is_empty()
            || effects
                .targets
                .iter()
                .any(|target| target.effect != crate::record::Effect::Name || target.glob);
        for cwd in scope
            .directory
            .alternatives
            .iter()
            .filter(|_| cwd_dependent)
        {
            let cwd = cwd.render();
            let mut copy = command.clone();
            copy.cwd = cwd.clone();
            for word in &mut copy.argv {
                if word.pwd {
                    word.reproject_cwd(&cwd);
                }
            }
            if !command.nested
                && command.pipeline.is_none()
                && !scope.piped
                && self.output.script.commands.contains(&copy)
            {
                continue;
            }
            if copy.program.is_some() && !copy.function {
                self.unresolved_calls.push((
                    self.output.script.commands.len(),
                    scope.defining,
                    self.namespace,
                ));
            }
            self.output.script.commands.push(copy);
        }
        if self.output.script.commands.len() > 512 {
            self.output.gap(CoverageGap::InspectionBudget);
        }
        command
    }

    fn command_coverage(&mut self, command: &Command, scope: &Scope) {
        if command.argv.iter().flat_map(|word| &word.vars).any(|name| {
            scope.bindings.get(name).is_some_and(|binding| {
                binding.values.iter().any(|value| {
                    matches!(
                        value,
                        BindingValue::RepeatedFields(_)
                            | BindingValue::RuntimeUnknown(_)
                            | BindingValue::RuntimeDerived(_)
                            | BindingValue::ShellDerived(_)
                    )
                })
            })
        }) && crate::targets::infer(command, &command.cwd, self.frontend.host)
            .targets
            .iter()
            .any(|target| target.expands)
        {
            self.output.gap(CoverageGap::UnresolvedTarget);
        }
        if let Some(gap) = &scope.directory.gap {
            // Only overflow pays for this second inference. A different cwd exposes
            // target dependencies without inventing a separate adapter role table.
            let effects = crate::targets::infer(command, &command.cwd, self.frontend.host);
            let relocated = crate::targets::infer(command, "/", self.frontend.host);
            if command.argv.iter().any(|word| word.pwd)
                || effects.targets != relocated.targets
                || !effects.code.is_empty()
                || !effects.inline.is_empty()
            {
                self.output.gap(gap.clone());
            }
        }
    }
}
