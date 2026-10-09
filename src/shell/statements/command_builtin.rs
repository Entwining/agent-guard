use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    fn printf_binding(&mut self, argv: &[crate::record::Word], index: usize, scope: &mut Scope) {
        if let Some(name) = argv.get(index + 2).filter(|word| identifier(&word.text)) {
            let values = &argv[index + 3..];
            if values.first().is_some_and(|word| word == "%s")
                && values.len() == 2
                && !values[1].expands
            {
                let binding = if values[1].cardinality_unknown {
                    scope.expanded_binding(&values[1], &values[1].text)
                } else {
                    BindingValue::Known(values[1].text.clone())
                };
                scope.assign(name.text.clone(), vec![binding]);
            } else {
                scope.assign(name.text.clone(), vec![BindingValue::RuntimeUnknown(None)]);
            }
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
            argv[index + 1].cardinality_unknown = false;
            argv[index + 1].field_count_unknown = false;
        }
        if argv[index + 1..]
            .iter()
            .any(|word| word.expands || word.cardinality_unknown)
        {
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
            if argv[index] == "let" {
                for word in &argv[index + 1..] {
                    self.arithmetic(&word.text, scope, depth)?;
                }
            }
            self.declaration(&argv[index..], scope);
            match argv[index].text.as_str() {
                "unset" => self.unset(&argv[index + 1..], scope),
                "set" if argv.get(index + 1).is_some_and(|word| word == "--") => {
                    self.positionals(&argv[index + 2..], scope);
                }
                "shift" => self.shift(&argv[index + 1..], scope),
                "printf" if argv.get(index + 1).is_some_and(|word| word == "-v") => {
                    self.printf_binding(argv, index, scope)
                }
                "break" | "continue" => {
                    let mut state = scope.state();
                    state.continue_loop = argv[index] == "continue";
                    let level = if argv.len() == index + 1 {
                        Some(1)
                    } else if argv.len() == index + 2 {
                        argv[index + 1].text.parse::<usize>().ok()
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
                "hash" => self.named_directory_changes(&argv[index + 1..], scope),
                "mapfile" | "readarray" => self.read_array_lines(&argv[index + 1..], scope),
                _ => {}
            }
        }
        Ok(executed_output)
    }
}
