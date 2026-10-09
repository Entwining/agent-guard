use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn track(&mut self, command: &Command, scope: &mut Scope) {
        let Some(index) = command.program else {
            return;
        };
        let program = command.argv[index].text.as_str();
        if !command.shell || !matches!(program, "cd" | "pushd") {
            return;
        }
        let args = &command.argv[index + 1..];
        let operand = args
            .iter()
            .position(|w| !w.starts_with('-') || program == "cd" && w == "-");
        if scope.directory.gap == Some(CoverageGap::IdentityBound)
            && operand.is_some_and(|index| !args[index].starts_with('/'))
        {
            // Relative movement cannot refine an unresolved cwd domain.
            return;
        }
        if operand.is_some_and(|index| {
            (args[index].globs || args[index].shell_matches) && !args[index].starts_with('/')
        }) {
            scope.relative_glob_moves += 1;
        }
        let mut home_targets = Vec::new();
        let target = if let Some(operand) = operand {
            args[operand].text.clone()
        } else if program == "cd"
            && args.iter().all(|w| {
                w == "--"
                    || w.strip_prefix('-').is_some_and(|flags| {
                        !flags.is_empty() && flags.bytes().all(|b| b"PLqs".contains(&b))
                    })
            })
        {
            let Some(binding) = scope.bindings.get("HOME") else {
                return;
            };
            for value in binding.values.iter() {
                if let BindingValue::Known(value) = value {
                    if !home_targets.contains(value) {
                        home_targets.push(value.clone());
                    }
                } else {
                    self.output.gap(CoverageGap::IdentityBound);
                }
            }
            let Some(target) = home_targets.first() else {
                return;
            };
            target.clone()
        } else {
            return;
        };
        let modes = args[..operand.unwrap_or(args.len())]
            .iter()
            .filter(|w| {
                w.strip_prefix('-').is_some_and(|flags| {
                    !flags.is_empty() && flags.bytes().all(|b| b.is_ascii_alphabetic())
                })
            })
            .flat_map(|w| w.bytes().filter(|b| matches!(b, b'L' | b'P')))
            .collect::<Vec<_>>();
        let physical = modes.contains(&b'P')
            && operand.is_none_or(|i| !args[i].expands && !args[i].globs && !args[i].shell_matches);
        let disputed = physical && modes.last() == Some(&b'L');
        let mut targets = if home_targets.is_empty() {
            vec![target.clone()]
        } else {
            home_targets
        };
        let oldpwd = program == "cd" && target == "-";
        if !self.directory_targets(program, args, scope, operand, &target, &mut targets) {
            return;
        }
        self.cdpath_targets(scope, operand, oldpwd, &target, &mut targets);
        scope.assign(
            "OLDPWD".into(),
            std::iter::once(&scope.directory.current)
                .chain(scope.directory.alternatives.iter())
                .map(|path| BindingValue::Known(path.render()))
                .collect(),
        );
        if program == "cd"
            && targets.iter().any(|target| {
                !target.starts_with('/')
                    && target
                        .split('/')
                        .any(|part| !["", ".", ".."].contains(&part))
            })
        {
            scope.directory.relative_growth += 1;
        }
        scope
            .directory
            .move_to(&targets, physical, disputed, self.frontend.host.home);
        Rc::make_mut(&mut scope.bindings).remove("PWD");
    }

    fn directory_targets(
        &mut self,
        program: &str,
        args: &[crate::record::Word],
        scope: &Scope,
        operand: Option<usize>,
        target: &str,
        targets: &mut Vec<String>,
    ) -> bool {
        let oldpwd = program == "cd" && target == "-";
        if oldpwd {
            targets.clear();
            if let Some(binding) = scope.bindings.get("OLDPWD") {
                for value in binding.values.iter() {
                    match value {
                        BindingValue::Known(value) => targets.push(value.clone()),
                        BindingValue::RepeatedFields(_)
                        | BindingValue::Arguments(_)
                        | BindingValue::Array(_)
                        | BindingValue::Undetermined => {
                            self.output.gap(CoverageGap::UnsupportedShellSyntax);
                        }
                        BindingValue::RuntimeUnknown(_)
                        | BindingValue::RuntimeDerived(_)
                        | BindingValue::ShellMatches(_)
                        | BindingValue::ShellDerived(_) => {}
                    }
                }
            }
            if targets.is_empty() {
                return false;
            }
        } else if program == "cd"
            && let Some(operand) = operand
            && args.len() == operand + 2
            && !args[operand].expands
            && !args[operand + 1].expands
        {
            let mut readings = std::iter::once(scope.directory.current.render())
                .chain(
                    scope
                        .directory
                        .alternatives
                        .iter()
                        .map(cwd::CwdPath::render),
                )
                .collect::<Vec<_>>();
            if let Some(binding) = scope.bindings.get("PWD") {
                for value in binding.values.iter() {
                    match value {
                        BindingValue::Known(value) => readings.push(value.clone()),
                        BindingValue::RepeatedFields(_)
                        | BindingValue::Arguments(_)
                        | BindingValue::Array(_)
                        | BindingValue::Undetermined => {
                            self.output.gap(CoverageGap::UnsupportedShellSyntax);
                        }
                        BindingValue::RuntimeUnknown(_)
                        | BindingValue::RuntimeDerived(_)
                        | BindingValue::ShellMatches(_)
                        | BindingValue::ShellDerived(_) => {}
                    }
                }
            }
            for reading in readings {
                if reading.contains(target) {
                    let replaced = reading.replacen(target, &args[operand + 1].text, 1);
                    if !targets.contains(&replaced) {
                        targets.push(replaced);
                    }
                }
            }
        }
        true
    }

    fn cdpath_targets(
        &mut self,
        scope: &Scope,
        operand: Option<usize>,
        oldpwd: bool,
        target: &str,
        targets: &mut Vec<String>,
    ) {
        if operand.is_some()
            && !oldpwd
            && !target.starts_with(['/', '.'])
            && let Some(binding) = scope.bindings.get("CDPATH")
        {
            for value in binding.values.iter() {
                if let BindingValue::Known(value) = value {
                    for entry in value.split(':').filter(|entry| !entry.is_empty()) {
                        let path = format!("{entry}/{target}");
                        if !targets.contains(&path) {
                            targets.push(path);
                        }
                    }
                } else if binding.bash_values.is_some()
                    && !matches!(value, BindingValue::RuntimeUnknown(Some(value)) if value.is_empty())
                {
                    self.output.gap(CoverageGap::IdentityBound);
                } else if matches!(
                    value,
                    BindingValue::RepeatedFields(_) | BindingValue::Undetermined
                ) {
                    self.output.gap(CoverageGap::UnsupportedShellSyntax);
                }
            }
        }
    }
}
