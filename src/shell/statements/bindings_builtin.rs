use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn unset(&mut self, argv: &[crate::record::Word], scope: &mut Scope) {
        let mut functions = false;
        let mut options = true;
        // After a word the shell has not fixed, neither the mode nor which
        // later words are operands is established: names are possibly unset.
        let mut certain = true;
        let mut complete = true;
        for word in argv {
            let Some(text) = word.exact() else {
                certain = false;
                // Only a word that cannot start an identifier names no variable.
                if !word.known_prefix().starts_with('-') {
                    complete &= scope.may_mutate(&BindingValue::Unset);
                }
                continue;
            };
            certain &= word.fixed_position;
            if options && text == "--" {
                options = false;
                continue;
            }
            if options && text.starts_with('-') {
                match text {
                    "-v" => functions = false,
                    "-f" => functions = true,
                    _ if certain => return,
                    _ => {}
                }
                continue;
            }
            options = false;
            if !identifier(text) {
                continue;
            }
            if !certain {
                complete &= scope.possibly_unset(text);
                if self.functions.contains_key(text) {
                    self.output.gap(CoverageGap::UnresolvedTarget);
                }
            } else if functions {
                if !scope.defining {
                    Rc::make_mut(&mut self.functions).remove(text);
                }
            } else {
                let prefix = format!("{text}[");
                Rc::make_mut(&mut scope.bindings).retain(|name, _| !name.starts_with(&prefix));
                scope.unset_binding(text);
            }
        }
        if !complete {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
        }
    }
}
