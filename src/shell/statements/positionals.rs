use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn shift(&mut self, args: &[crate::record::Word], scope: &mut Scope) {
        if scope.defining
            && scope
                .bindings
                .get("#")
                .is_some_and(|binding| binding.values.iter().any(|value| value.known().is_none()))
        {
            return;
        }
        let amount = if args.is_empty() {
            Some(1)
        } else {
            args.first()
                .filter(|word| word.fixed_position)
                .and_then(|word| word.exact())
                .and_then(|text| text.parse::<usize>().ok())
        };
        if let Some(sequences) = scope.positional_sequences() {
            if amount.is_none() {
                self.output.gap(CoverageGap::UnresolvedTarget);
            }
            let before = scope.clone();
            let mut exits = Vec::new();
            for arguments in sequences {
                // An unknown amount shifts by any count; one larger than the
                // parameter count fails and shifts nothing.
                let amounts = amount.map_or(0..=arguments.len(), |amount| amount..=amount);
                for amount in amounts {
                    let mut branch = before.clone();
                    if amount <= arguments.len() {
                        self.positionals(&arguments[amount..], &mut branch);
                    }
                    exits.push(branch);
                }
            }
            self.merge_bindings(scope, &exits);
        } else if scope.bindings.contains_key("#") {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
        } else {
            self.output.gap(CoverageGap::UnresolvedTarget);
        }
    }
    pub(super) fn set(&mut self, args: &[crate::record::Word], scope: &mut Scope) {
        // The first argument that is not an option starts the new positional
        // parameters; `--` replaces them even with none, `-` only with some.
        let start = |certain: bool| {
            let mut index = 0;
            while let Some(word) = args.get(index) {
                let text = if certain {
                    word.exact().filter(|_| word.fixed_position)?
                } else {
                    &word.text
                };
                if text == "--" {
                    return Some(Some(index + 1));
                }
                if text == "-" {
                    return Some((index + 1 < args.len()).then_some(index + 1));
                }
                if text.len() > 1 && text.starts_with(['-', '+']) {
                    if text.contains('A') {
                        // Zsh array assignment is not a positional update.
                        return Some(None);
                    }
                    index += 1 + usize::from(text.contains('o'));
                    continue;
                }
                return Some(Some(index));
            }
            Some(None)
        };
        match start(true) {
            Some(Some(index)) => self.positionals(&args[index..], scope),
            Some(None) => {}
            None => {
                let before = scope.clone();
                let mut replaced = scope.clone();
                if let Some(Some(index)) = start(false) {
                    self.positionals(&args[index..], &mut replaced);
                }
                self.merge_bindings(scope, &[before, replaced]);
                scope.positionals_unknown();
            }
        }
    }
    pub(super) fn positionals(&mut self, argv: &[crate::record::Word], scope: &mut Scope) {
        let bindings = argv
            .iter()
            .map(|word| scope.expanded_binding(word, &word.text))
            .collect::<Vec<_>>();
        let mut arguments = argv.to_vec();
        for (index, (word, binding)) in arguments.iter_mut().zip(&bindings).enumerate() {
            let name = (index + 1).to_string();
            word.raw = format!("${{{name}}}");
            word.vars = vec![name.clone()];
            word.binding_candidates = BTreeMap::from([(name, word.text.clone())]);
            word.runtime_unknown |= matches!(
                binding,
                BindingValue::RuntimeUnknown(_) | BindingValue::RuntimeDerived(_)
            );
            word.shell_matches |= matches!(
                binding,
                BindingValue::ShellMatches(_) | BindingValue::ShellDerived(_)
            );
        }
        scope.local("@");
        scope.assign("@".into(), vec![BindingValue::Arguments(arguments)]);
        scope.local("#");
        scope.assign(
            "#".into(),
            vec![if argv.iter().any(|word| word.field_count_unknown) {
                BindingValue::RuntimeDerived(argv.len().to_string().into())
            } else {
                BindingValue::Known(argv.len().to_string())
            }],
        );
        let prior = scope
            .bindings
            .keys()
            .filter(|name| name.parse::<usize>().is_ok())
            .cloned()
            .collect::<Vec<_>>();
        for name in prior {
            scope.local(&name);
            Rc::make_mut(&mut scope.bindings).remove(&name);
        }
        for (index, (word, binding)) in argv.iter().zip(bindings).enumerate() {
            let name = (index + 1).to_string();
            scope.local(&name);
            scope.assign(
                name,
                vec![
                    if word.shell_matches && !matches!(binding, BindingValue::RepeatedFields(_)) {
                        if word.expands {
                            BindingValue::ShellDerived(ShellValue::from_word(word))
                        } else {
                            BindingValue::ShellMatches(ShellValue::from_word(word))
                        }
                    } else {
                        binding
                    },
                ],
            );
        }
    }
}
