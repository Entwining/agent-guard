use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
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
                let prefix = format!("{}[", word.text);
                Rc::make_mut(&mut scope.bindings)
                    .retain(|name, _| name != &word.text && !name.starts_with(&prefix));
            }
        }
    }
}
