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
        let mut child = Scope::new(self.frontend.host.home, &command.cwd);
        child.pipeline_input = scope.pipeline_input.clone();
        child.input_fds = scope.input_fds.clone();
        child.stdin_id = scope.stdin_id;
        child.input_cursors = scope.input_cursors.clone();
        child.captured = scope.captured;
        child.output_fds = scope.output_fds.clone();
        child.flow_guard = scope.flow_guard.clone();
        child.zsh = self.frontend.zsh;
        Rc::make_mut(&mut child.bindings).extend(
            scope
                .bindings
                .iter()
                .filter(|(name, binding)| {
                    (binding.exported || prefixes.contains_key(*name))
                        && !matches!(name.as_str(), "GIT_DIR" | "GIT_WORK_TREE")
                })
                .map(|(name, binding)| (name.clone(), binding.clone())),
        );
        for (name, value) in &command.environment {
            let value = if value.expands {
                BindingValue::RuntimeDerived(value.text.clone())
            } else {
                BindingValue::Known(value.text.clone())
            };
            let binding = Rc::make_mut(&mut child.bindings)
                .entry(name.clone())
                .or_insert_with(|| Binding {
                    #[cfg(test)]
                    copies: EntryCopies::default(),
                    origins: None,
                    bash_values: None,
                    values: Rc::default(),
                    exported: true,
                    arithmetic: false,
                });
            if !binding.values.contains(&value) {
                Rc::make_mut(&mut binding.values).push(value);
            }
        }
        for change in environment {
            match change {
                shell::argv::EnvironmentChange::Clear => Rc::make_mut(&mut child.bindings).clear(),
                shell::argv::EnvironmentChange::Unset(name) => {
                    Rc::make_mut(&mut child.bindings).remove(name);
                    if child.zsh
                        && let Some(alias) = cdpath_alias(name)
                    {
                        Rc::make_mut(&mut child.bindings).remove(alias);
                    }
                }
                shell::argv::EnvironmentChange::Set(name, value) => {
                    child.assign(
                        name.clone(),
                        vec![scope.expanded_binding(value, &value.text)],
                    );
                    if let Some(binding) = Rc::make_mut(&mut child.bindings).get_mut(name) {
                        binding.exported = true;
                    }
                }
            }
        }
        child
    }
}
