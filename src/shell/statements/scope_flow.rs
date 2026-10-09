use super::*;

impl Scope {
    pub(super) fn forget_choice(&mut self, choice: usize) {
        Rc::make_mut(&mut self.flow_guard).remove(&choice);
        if !self.bindings.values().any(|binding| {
            binding.origins.as_ref().is_some_and(|origins| {
                origins
                    .values()
                    .any(|guards| guards.iter().any(|guard| guard.contains_key(&choice)))
            })
        }) {
            return;
        }
        for binding in Rc::make_mut(&mut self.bindings).values_mut() {
            if let Some(origins) = &mut binding.origins {
                for guards in Rc::make_mut(origins).values_mut() {
                    for guard in guards {
                        guard.remove(&choice);
                    }
                }
            }
        }
    }
    pub(in crate::shell) fn capture(&mut self, channel: usize) {
        self.captured = true;
        Rc::make_mut(&mut self.output_fds).insert(1, Some(channel));
    }
    pub(in crate::shell) fn input(&self, fd: i32) -> Option<&Flow> {
        let id = if fd == 0 {
            self.stdin_id
        } else {
            *self.input_fds.get(&fd)?
        };
        self.input_cursors.get(&id).or({
            if fd == 0 {
                self.pipeline_input.as_ref()
            } else {
                None
            }
        })
    }
    pub(super) fn restore_inputs(
        &mut self,
        stdin: usize,
        input: Option<Flow>,
        fds: Rc<BTreeMap<i32, usize>>,
    ) {
        self.stdin_id = stdin;
        self.input_fds = fds;
        self.pipeline_input = self.input_cursors.get(&stdin).cloned().or(input);
    }
    pub(super) fn word_guards(
        &self,
        word: &crate::record::Word,
        flow: &mut FlowBuilder,
    ) -> Vec<Guard> {
        if self.flow_end {
            return Vec::new();
        }
        let mut guards = vec![self.flow_guard.as_ref().clone()];
        for (name, value) in &word.binding_candidates {
            let Some(origins) = self
                .bindings
                .get(name)
                .and_then(|binding| binding.origins.as_ref())
                .and_then(|origins| origins.get(value))
            else {
                continue;
            };
            guards = flow.guards(&guards, origins);
        }
        guards
    }
    pub(super) fn value_origins(&self, values: &[Expanded], flow: &mut FlowBuilder) -> Origins {
        let mut origins = Origins::new();
        for value in values {
            origins
                .entry(value.word.text.clone())
                .or_default()
                .extend(self.word_guards(&value.word, flow));
        }
        origins
    }
    pub(super) fn set_origins(&mut self, name: &str, origins: Origins) {
        if self.captured
            && let Some(binding) = Rc::make_mut(&mut self.bindings).get_mut(name)
        {
            binding.origins = Some(Rc::new(origins));
        }
    }
}
