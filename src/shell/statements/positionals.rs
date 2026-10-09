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
                .filter(|word| !word.expands)
                .and_then(|word| word.parse::<usize>().ok())
        };
        if let (Some(amount), Some(sequences)) = (amount, scope.positional_sequences()) {
            let before = scope.clone();
            let mut exits = Vec::new();
            for arguments in sequences {
                let mut branch = before.clone();
                if amount <= arguments.len() {
                    self.positionals(&arguments[amount..], &mut branch);
                }
                exits.push(branch);
            }
            self.merge_bindings(scope, &exits);
        } else if scope.bindings.contains_key("#") {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
        } else {
            self.output.gap(CoverageGap::UnresolvedTarget);
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
                BindingValue::RuntimeDerived(argv.len().to_string())
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
                vec![if matches!(binding, BindingValue::RepeatedFields(_)) {
                    binding
                } else if word.shell_matches && word.expands {
                    BindingValue::ShellDerived(ShellValue::from_word(word))
                } else if word.shell_matches {
                    BindingValue::ShellMatches(ShellValue::from_word(word))
                } else if word.expands {
                    BindingValue::RuntimeUnknown(None)
                } else {
                    BindingValue::Known(word.text.clone())
                }],
            );
        }
    }
}
