use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn named_directory_changes(
        &mut self,
        args: &[crate::record::Word],
        scope: &mut Scope,
    ) {
        if !scope.zsh
            || !args.iter().any(|arg| {
                arg.strip_prefix('-')
                    .is_some_and(|flags| flags.contains('d'))
            })
        {
            return;
        }
        if args.iter().any(|arg| {
            arg.expands
                || arg.cardinality_unknown
                || arg.globs
                || arg.shell_matches
                || arg.runtime_unknown
        }) {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
            return;
        }
        if args.iter().any(|arg| {
            arg.strip_prefix('-')
                .is_some_and(|flags| flags.contains('f'))
        }) {
            self.output.gap(CoverageGap::IdentityBound);
            return;
        }
        if args.iter().any(|arg| {
            arg.strip_prefix('-')
                .is_some_and(|flags| flags.contains('r'))
        }) {
            scope.named_dirs = std::rc::Rc::new(BTreeMap::new());
        }
        for arg in args.iter().filter(|arg| !arg.starts_with('-')) {
            if let Some((name, value)) = arg.split_once('=') {
                if name.is_empty()
                    || !name
                        .bytes()
                        .all(|ch| ch.is_ascii_alphanumeric() || b"_.-".contains(&ch))
                {
                    self.output.gap(CoverageGap::UnsupportedShellSyntax);
                    continue;
                }
                if !scope.named_dirs.get(name).is_some_and(|values| {
                    matches!(values.as_slice(), [BindingValue::Known(old)] if old == value)
                }) {
                    std::rc::Rc::make_mut(&mut scope.named_dirs)
                        .insert(name.into(), vec![BindingValue::Known(value.into())]);
                }
            } else if !scope.named_dirs.contains_key(&arg.text) {
                std::rc::Rc::make_mut(&mut scope.named_dirs)
                    .insert(arg.text.clone(), vec![BindingValue::RuntimeUnknown(None)]);
            }
        }
    }

    pub(super) fn unset(&mut self, argv: &[crate::record::Word], scope: &mut Scope) {
        let mut functions = false;
        let mut options = true;
        for word in argv {
            if options && word == "--" {
                options = false;
                continue;
            }
            if options && word.starts_with('-') {
                match word.text.as_str() {
                    "-v" => functions = false,
                    "-f" => functions = true,
                    _ => return,
                }
                continue;
            }
            options = false;
            if !identifier(&word.text) || scope.expanded_binding(word, &word.text).known().is_none()
            {
                continue;
            }
            if functions {
                if !scope.defining {
                    Rc::make_mut(&mut self.functions).remove(&word.text);
                }
            } else {
                if scope.zsh
                    && let Some(alias) = cdpath_alias(&word.text)
                    && let Some(binding) = Rc::make_mut(&mut scope.bindings).get_mut(alias)
                {
                    binding.values = Rc::new(shell::arrays::bash_only(
                        binding
                            .bash_values
                            .as_deref()
                            .cloned()
                            .unwrap_or_else(|| binding.values.as_ref().clone()),
                    ));
                    Rc::make_mut(&mut binding.values)
                        .push(BindingValue::RuntimeUnknown(Some(String::new())));
                }
                let prefix = format!("{}[", word.text);
                Rc::make_mut(&mut scope.bindings)
                    .retain(|name, _| name != &word.text && !name.starts_with(&prefix));
            }
        }
    }
}
