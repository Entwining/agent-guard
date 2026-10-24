use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    fn printf_binding(&mut self, argv: &[crate::record::Word], index: usize, scope: &mut Scope) {
        let Some(option) = argv.get(index + 1) else {
            return;
        };
        // Only a first argument of `-v` selects assignment.
        let assigns = option.exact() == Some("-v") && option.fixed_position;
        if !assigns && option.exact().is_some() && option.fixed_position {
            return;
        }
        let values = argv.get(index + 3..).unwrap_or_default();
        // A quoted repetition is still one argument; its count is the value's.
        let value = if values.len() == 2
            && values[0].exact() == Some("%s")
            && values[0].fixed_position
            && (values[1].fixed_position
                || values[1].cardinality_unknown && !values[1].field_count_unknown)
        {
            scope.expanded_binding(&values[1], &values[1].text)
        } else {
            BindingValue::RuntimeUnknown(None)
        };
        let complete = match argv.get(index + 2) {
            Some(name)
                if option.fixed_position && name.fixed_position && name.exact().is_some() =>
            {
                let name = &name.text;
                if !identifier(name) {
                    true
                } else if assigns {
                    scope.assign(name.clone(), vec![value]);
                    true
                } else {
                    scope.possibly_assign(name, &[value])
                }
            }
            Some(_) => scope.may_mutate(&value),
            None => true,
        };
        if !complete {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
        }
    }

    /// The state after a command whose identity the shell has not fixed,
    /// which may have run in this shell: every tracked variable, the
    /// positional parameters and the cwd keep their states and may also be
    /// unknown, and defined functions may have changed.
    fn unresolved_identity(&mut self, scope: &mut Scope) {
        if !scope.may_mutate(&BindingValue::Inherited) {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
        }
        scope.positionals_unknown();
        scope.directory.possibly_moved();
        if !self.functions.is_empty() {
            self.output.gap(CoverageGap::UnresolvedTarget);
        }
    }

    fn eval_builtin(
        &mut self,
        argv: &mut [crate::record::Word],
        index: usize,
        scope: &mut Scope,
        prior: &BTreeMap<String, Option<Binding>>,
        depth: usize,
    ) -> Result<Option<Output>, CheckError> {
        let mut output = None;
        let source = if argv.len() == index + 2 {
            scope
                .repetition_source(&argv[index + 1])
                .map(|source| Box::new(source.1))
        } else {
            None
        };
        if source.is_some() {
            // The source below re-expands the repetition with its binding.
            argv[index + 1].role = Role::ObservedShellCode;
        }
        if source.is_none()
            && argv[index + 1..]
                .iter()
                .any(|word| word.expands || word.cardinality_unknown || word.exact().is_none())
        {
            // A representative is lexical evidence of the code, which the
            // target owner scans, not code this shell runs.
            self.unresolved_identity(scope);
            self.output.gap(
                if argv[index + 1..].iter().any(|word| {
                    word.cardinality_unknown
                        || word.expands && !word.runtime_unknown && !word.vars.is_empty()
                }) || !scope.frames.is_empty()
                    || !prior.is_empty()
                {
                    CoverageGap::UnsupportedShellSyntax
                } else {
                    CoverageGap::UnresolvedTarget
                },
            );
        } else {
            let before = scope.state();
            let code = source.unwrap_or_else(|| {
                argv[index + 1..]
                    .iter()
                    .map(|word| word.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
                    .into()
            });
            output = Some(self.source(&code, scope, depth + 1)?);
            // Target inspection alone does not establish binding restoration after eval.
            if !scope.frames.is_empty() || !prior.is_empty() || scope.state() != before {
                self.output.gap(CoverageGap::UnsupportedShellSyntax);
            }
        }
        Ok(output)
    }

    pub(super) fn command_builtin(
        &mut self,
        argv: &mut [crate::record::Word],
        resolved: &shell::argv::Resolution,
        scope: &mut Scope,
        prior: &BTreeMap<String, Option<Binding>>,
        depth: usize,
    ) -> Result<Option<Output>, CheckError> {
        let program = resolved.program;
        let mut executed_output = None;
        if let Some(index) = program.filter(|i| {
            !self.functions.contains_key(&argv[*i].text)
                && (resolved.shell
                    || argv[*i] == "eval" && resolved.wrappers.iter().any(|w| w == "command"))
        }) {
            if argv[index].expands && !self.functions.is_empty() && !scope.defining {
                self.output.gap(CoverageGap::UnsupportedShellSyntax);
            }
            if argv[index].exact().is_none() {
                self.unresolved_identity(scope);
                return Ok(None);
            }
            if argv[index] == "let" {
                for word in &argv[index + 1..] {
                    self.arithmetic(&word.text, scope, depth)?;
                }
            }
            self.declaration(&argv[index..], scope);
            match argv[index].text.as_str() {
                "unset" => self.unset(&argv[index + 1..], scope),
                "set" => self.set(&argv[index + 1..], scope),
                "shift" => self.shift(&argv[index + 1..], scope),
                "printf" => self.printf_binding(argv, index, scope),
                "break" | "continue" => {
                    let mut state = scope.state();
                    state.continue_loop = argv[index] == "continue";
                    let level = if argv.len() == index + 1 {
                        Some(1)
                    } else if argv.len() == index + 2 && argv[index + 1].fixed_position {
                        argv[index + 1]
                            .exact()
                            .and_then(|text| text.parse::<usize>().ok())
                    } else {
                        None
                    };
                    if level == Some(1)
                        && prior.is_empty()
                        && let Some(exits) = Rc::make_mut(&mut scope.loops).last_mut()
                    {
                        Rc::make_mut(exits).push(state);
                        scope.flow_end = true;
                    } else {
                        self.output.gap(CoverageGap::UnsupportedShellSyntax);
                    }
                }
                "return" => {
                    if scope.frames.is_empty() {
                        self.output.gap(CoverageGap::UnsupportedShellSyntax);
                    } else {
                        let state = scope.state();
                        Rc::make_mut(&mut scope.returns).push(state);
                        scope.flow_end = true;
                    }
                }
                "eval" => executed_output = self.eval_builtin(argv, index, scope, prior, depth)?,
                "read" => self.read(&argv[index + 1..], scope),
                "mapfile" | "readarray" => self.read_array_lines(&argv[index + 1..], scope),
                _ => {}
            }
        }
        Ok(executed_output)
    }
}
