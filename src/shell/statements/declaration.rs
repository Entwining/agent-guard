use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn declaration(&mut self, argv: &[crate::record::Word], scope: &mut Scope) {
        let Some(program) = argv.first().map(|w| w.text.as_str()) else {
            return;
        };
        if !matches!(program, "export" | "local" | "declare" | "typeset") {
            return;
        }
        if self.function_exports(argv, program) {
            return;
        }
        let in_function = !scope.frames.is_empty();
        let local = in_function && matches!(program, "local" | "declare" | "typeset");
        let top_local = !in_function && program == "local";
        let exported = if program == "export" {
            Some(if argv.iter().any(|word| word == "-n") {
                Export::No
            } else {
                Export::Yes
            })
        } else {
            argv.iter()
                .skip(1)
                .filter_map(|word| {
                    word.strip_prefix('-')
                        .map(|flags| (flags, Export::Yes))
                        .or_else(|| word.strip_prefix('+').map(|flags| (flags, Export::No)))
                })
                .filter_map(|(flags, export)| flags.contains('x').then_some(export))
                .next_back()
        };
        let arithmetic = argv
            .iter()
            .skip(1)
            .filter_map(|word| {
                let flag = word
                    .strip_prefix('-')
                    .map(|value| (value, true))
                    .or_else(|| word.strip_prefix('+').map(|value| (value, false)))?;
                flag.0.contains('i').then_some(flag.1)
            })
            .next_back();
        let indexed = argv.iter().skip(1).any(|word| {
            word.strip_prefix('-')
                .is_some_and(|flags| flags.contains('a'))
        });
        if in_function
            && (program == "export"
                || argv.iter().skip(1).any(|word| {
                    word.strip_prefix('-')
                        .is_some_and(|flags| flags.chars().any(|flag| flag != 'a'))
                }))
        {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
        }
        // A word whose option or variable name the shell has not fixed makes
        // every declaration here possible rather than established.
        let before = argv[1..]
            .iter()
            .any(|word| word.exact().is_none() && assignment(word.known_prefix()).is_none())
            .then(|| scope.clone());
        for word in &argv[1..] {
            self.declaration_word(word, scope, (local, top_local, exported, indexed));
            if let Some(arithmetic) = arithmetic {
                let name = assignment(&word.text).map_or(word.text.as_str(), |(name, _)| name);
                if identifier(name) {
                    let absent = scope.absent();
                    Rc::make_mut(&mut scope.bindings)
                        .entry(name.into())
                        .or_insert_with(|| Binding {
                            #[cfg(test)]
                            copies: EntryCopies::default(),
                            origins: None,
                            values: Rc::new(vec![absent]),
                            exported: Export::No,
                            arithmetic: false,
                        })
                        .arithmetic = arithmetic;
                }
            }
        }
        if let Some(before) = before {
            let after = scope.clone();
            self.merge_bindings(scope, &[before, after]);
            if !scope.may_mutate(&BindingValue::Inherited) {
                self.output.gap(CoverageGap::UnsupportedShellSyntax);
            }
        }
    }

    fn function_exports(&mut self, argv: &[crate::record::Word], program: &str) -> bool {
        if argv.iter().any(|word| {
            word.strip_prefix('-')
                .is_some_and(|flags| flags.contains('f'))
        }) && matches!(program, "export" | "declare" | "typeset")
        {
            let exported = if program == "export" {
                Some(!argv.iter().any(|word| {
                    word.strip_prefix('-')
                        .is_some_and(|flags| flags.contains('n'))
                }))
            } else {
                argv.iter()
                    .filter_map(|word| {
                        word.strip_prefix('-')
                            .map(|flags| (flags, true))
                            .or_else(|| word.strip_prefix('+').map(|flags| (flags, false)))
                    })
                    .filter_map(|(flags, enabled)| flags.contains('x').then_some(enabled))
                    .next_back()
            };
            if let Some(exported) = exported {
                for word in argv
                    .iter()
                    .skip(1)
                    .filter(|word| !word.starts_with(['-', '+']))
                {
                    if let Some(function) = Rc::make_mut(&mut self.functions).get_mut(&word.text) {
                        function.exported = exported;
                    }
                }
            }
            // Text the shell does not fix only possibly selects functions, so
            // its variable declarations stay possible too.
            return argv.iter().any(|word| {
                word.exact()
                    .and_then(|text| text.strip_prefix('-'))
                    .is_some_and(|flags| flags.contains('f'))
            });
        }
        false
    }

    fn declaration_word(
        &mut self,
        word: &crate::record::Word,
        scope: &mut Scope,
        flags: (bool, bool, Option<Export>, bool),
    ) {
        let (local, top_local, exported, indexed) = flags;
        if let Some((name, value)) = assignment(&word.text) {
            if local {
                scope.local(name);
            }
            let mut values = vec![scope.expanded_binding(word, value)];
            if top_local {
                let prior = scope.bindings.get(name).map_or_else(
                    || vec![BindingValue::Undetermined],
                    |b| b.values.as_ref().clone(),
                );
                for value in prior {
                    if !values.contains(&value) {
                        values.push(value);
                    }
                }
                if values.contains(&BindingValue::Undetermined) {
                    self.output.gap(CoverageGap::UnsupportedShellSyntax);
                }
            }
            if indexed
                && !scope.bindings.get(name).is_some_and(|binding| {
                    binding
                        .values
                        .iter()
                        .any(|value| matches!(value, BindingValue::Array(_)))
                })
            {
                values = vec![BindingValue::Array(Box::new(
                    shell::arrays::IndexedArray::scalar(self.frontend.zsh, &values),
                ))];
            }
            scope.assign(name.into(), values);
            if let Some(exported) = exported
                && let Some(binding) = Rc::make_mut(&mut scope.bindings).get_mut(name)
            {
                binding.exported = exported;
            }
        } else if let Some(exported) = exported.filter(|_| identifier(&word.text)) {
            let absent = scope.absent();
            Rc::make_mut(&mut scope.bindings)
                .entry(word.text.clone())
                .or_insert_with(|| Binding {
                    #[cfg(test)]
                    copies: EntryCopies::default(),
                    origins: None,
                    values: Rc::new(vec![absent]),
                    exported,
                    arithmetic: false,
                })
                .exported = exported;
        } else if local && identifier(&word.text) {
            let initialized_array = scope
                .frames
                .last()
                .is_some_and(|frame| frame.contains_key(&word.text))
                && scope.bindings.get(&word.text).is_some_and(|binding| {
                    binding
                        .values
                        .iter()
                        .any(|value| matches!(value, BindingValue::Array(_)))
                });
            if !initialized_array {
                scope.local(&word.text);
                // Depending on its release and on an outer variable, Bash
                // leaves a local without a value unset or sets it empty.
                let values = if self.frontend.zsh {
                    vec![BindingValue::Known(String::new())]
                } else {
                    vec![BindingValue::Known(String::new()), BindingValue::Unset]
                };
                scope.assign(word.text.clone(), values);
            }
        }
        if indexed
            && identifier(&word.text)
            && !scope.bindings.get(&word.text).is_some_and(|binding| {
                binding
                    .values
                    .iter()
                    .any(|value| matches!(value, BindingValue::Array(_)))
            })
        {
            scope.assign(
                word.text.clone(),
                vec![BindingValue::Array(Box::new(
                    shell::arrays::IndexedArray::scalar(
                        self.frontend.zsh,
                        &scope
                            .bindings
                            .get(&word.text)
                            .map_or_else(Vec::new, |binding| binding.values.as_ref().clone()),
                    ),
                ))],
            );
        }
    }
}
