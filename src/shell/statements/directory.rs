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
        let targets = if home_targets.is_empty() {
            vec![target]
        } else {
            home_targets
        };
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
        let bindings = Rc::make_mut(&mut scope.bindings);
        bindings.remove("PWD");
        bindings.remove("OLDPWD");
    }
}
