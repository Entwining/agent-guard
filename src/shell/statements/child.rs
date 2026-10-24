use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn shell_source(
        &mut self,
        source: &str,
        scope: &mut Scope,
        environment: &[shell::argv::EnvironmentChange],
        depth: usize,
    ) -> Result<Output, CheckError> {
        let functions = if environment
            .iter()
            .any(|change| matches!(change, shell::argv::EnvironmentChange::Clear))
        {
            Rc::default()
        } else {
            Rc::new(
                self.functions
                    .iter()
                    .filter(|(_, function)| function.exported)
                    .map(|(name, function)| (name.clone(), function.clone()))
                    .collect(),
            )
        };
        let parent_functions = std::mem::replace(&mut self.functions, functions);
        let namespace = self.function_namespaces.len();
        self.function_namespaces.push(self.functions.clone());
        let parent_namespace = self.namespace.replace(namespace);
        let result = self.source(source, scope, depth);
        self.function_namespaces[namespace] =
            std::mem::replace(&mut self.functions, parent_functions);
        self.namespace = parent_namespace;
        result
    }

    pub(super) fn shell_child(
        &self,
        command: &Command,
        scope: &Scope,
        prefixes: &BTreeMap<String, Option<Binding>>,
        environment: &[shell::argv::EnvironmentChange],
    ) -> Scope {
        let mut child = Scope::for_host(self.frontend.host, &command.cwd);
        child.pipeline_input = scope.pipeline_input.clone();
        child.input_fds = scope.input_fds.clone();
        child.stdin_id = scope.stdin_id;
        child.input_cursors = scope.input_cursors.clone();
        child.captured = scope.captured;
        child.output_fds = scope.output_fds.clone();
        child.flow_guard = scope.flow_guard.clone();
        // The named shell, not the one running this command, owns the
        // child's dialect.
        child.zsh = command
            .program
            .is_some_and(|index| command.argv[index].rsplit('/').next() == Some("zsh"));
        // The child imports what this shell exports. A variable this shell
        // unset is absent from its environment, and one the shell may not
        // have exported may hold only what the shell itself inherited.
        let absent = scope.absent();
        for (name, binding) in scope.bindings.iter() {
            let export = if prefixes.contains_key(name)
                || binding
                    .values
                    .iter()
                    .all(|value| value == &BindingValue::Unset)
            {
                Export::Yes
            } else {
                binding.exported
            };
            let mut binding = binding.clone();
            match export {
                Export::Yes => {}
                // Neither shell passes an array to a command.
                Export::Unknown
                    if !binding
                        .values
                        .iter()
                        .any(|value| matches!(value, BindingValue::Array(_))) =>
                {
                    if !binding.values.contains(&absent) {
                        Rc::make_mut(&mut binding.values).push(absent.clone());
                    }
                }
                Export::Unknown | Export::No => continue,
            }
            binding.exported = Export::Yes;
            Rc::make_mut(&mut child.bindings).insert(name.clone(), binding);
        }
        child.inherited_unset = scope.inherited_unset;
        for change in environment {
            match change {
                shell::argv::EnvironmentChange::Clear => {
                    Rc::make_mut(&mut child.bindings).clear();
                    child.inherited_unset = true;
                }
                shell::argv::EnvironmentChange::Unset(name) => child.unset_binding(name),
                shell::argv::EnvironmentChange::Set(name, value) => {
                    child.assign(
                        name.clone(),
                        vec![scope.expanded_binding(value, &value.text)],
                    );
                    if let Some(binding) = Rc::make_mut(&mut child.bindings).get_mut(name) {
                        binding.exported = Export::Yes;
                    }
                }
            }
        }
        child
    }
}
