use super::*;

impl Scope {
    pub(in crate::shell) fn named_contexts(
        &self,
        names: &BTreeSet<String>,
    ) -> (Vec<BTreeMap<String, Option<String>>>, bool) {
        let mut contexts = vec![BTreeMap::new()];
        for name in names {
            if let Some(values) = self.named_dirs.get(name) {
                let mut next = Vec::new();
                for context in &contexts {
                    for value in values {
                        let mut context = context.clone();
                        context.insert(name.clone(), value.known().cloned());
                        if !next.contains(&context) {
                            if next.len() == 512 {
                                return (next, true);
                            }
                            next.push(context);
                        }
                    }
                }
                contexts = next;
            }
        }
        (contexts, false)
    }
    pub(in crate::shell) fn in_function(&self) -> bool {
        !self.frames.is_empty()
    }
    pub(in crate::shell) fn ifs_candidates(&self) -> Vec<Option<String>> {
        self.bindings.get("IFS").map_or_else(
            || vec![None],
            |binding| {
                binding
                    .values
                    .iter()
                    .map(|value| value.known().cloned())
                    .collect()
            },
        )
    }
    pub fn new(home: &str, cwd: &str) -> Self {
        Self {
            #[cfg(test)]
            candidate_work: Rc::default(),
            named_dirs: std::rc::Rc::new(BTreeMap::new()),
            directory: Directory::new(cwd),
            bindings: Rc::new(BTreeMap::from([(
                "HOME".into(),
                Binding {
                    #[cfg(test)]
                    copies: EntryCopies::default(),
                    origins: None,
                    bash_values: None,
                    values: Rc::new(vec![BindingValue::Known(home.into())]),
                    exported: false,
                    arithmetic: false,
                },
            )])),
            frames: Rc::default(),
            isolated: false,
            defining: false,
            conditional_definition: false,
            returns: Rc::default(),
            loops: Rc::default(),
            summarizing_loop: false,
            bounded_loop: false,
            conditional_append: false,
            piped: false,
            zsh: false,
            pipeline_input: None,
            captured: false,
            output_fds: Rc::new(BTreeMap::from([(1, None), (2, None)])),
            input_fds: Rc::default(),
            stdin_id: usize::MAX,
            input_cursors: Rc::default(),
            flow_guard: Rc::default(),
            flow_end: false,
            relative_glob_moves: 0,
        }
    }
    pub fn isolated(&self) -> Self {
        let mut child = self.clone();
        child.returns = Rc::default();
        child.loops = Rc::default();
        child.summarizing_loop = false;
        child.bounded_loop = false;
        child.conditional_append = false;
        child.isolated = true;
        child.directory.failures = None;
        child
    }
    pub(super) fn branch(&self) -> Self {
        let mut child = self.clone();
        child.conditional_definition = true;
        child
    }
    pub(super) fn same_values_and_input(&self, other: &Self) -> bool {
        self.bindings.len() == other.bindings.len()
            && self.bindings.iter().all(|(name, binding)| {
                other.bindings.get(name).is_some_and(|other| {
                    binding.values == other.values
                        && binding.bash_values == other.bash_values
                        && binding.exported == other.exported
                        && binding.arithmetic == other.arithmetic
                })
            })
            && self.directory.current == other.directory.current
            && self.directory.alternatives == other.directory.alternatives
            && self.input(0) == other.input(0)
            && self.stdin_id == other.stdin_id
            && self.input_fds == other.input_fds
            && self
                .input_fds
                .keys()
                .all(|fd| self.input(*fd) == other.input(*fd))
    }
    pub(super) fn same_flow_state(&self, other: &Self) -> bool {
        self.same_values_and_input(other)
            && self.flow_end == other.flow_end
            && Rc::ptr_eq(&self.returns, &other.returns)
            && Rc::ptr_eq(&self.loops, &other.loops)
    }
    pub(super) fn state(&self) -> BindingState {
        BindingState {
            continue_loop: false,
            flow_guard: self.flow_guard.clone(),
            named_dirs: self.named_dirs.clone(),
            bindings: self.bindings.clone(),
            frames: self.frames.clone(),
        }
    }
    pub(super) fn with_state(&self, state: BindingState) -> Self {
        let mut scope = self.clone();
        scope.flow_guard = state.flow_guard;
        scope.named_dirs = state.named_dirs;
        scope.bindings = state.bindings;
        scope.frames = state.frames;
        scope
    }
    pub(super) fn enter_function(&mut self) {
        Rc::make_mut(&mut self.frames).push(Rc::default());
    }
    pub(super) fn leave_function(&mut self) {
        if let Some(frame) = Rc::make_mut(&mut self.frames).pop() {
            for (name, prior) in Rc::unwrap_or_clone(frame) {
                if let Some(prior) = prior {
                    Rc::make_mut(&mut self.bindings).insert(name, prior);
                } else {
                    Rc::make_mut(&mut self.bindings).remove(&name);
                }
            }
        }
    }
}
