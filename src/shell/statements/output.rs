use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn redirect_fds(&mut self, scope: &mut Scope, targets: &[crate::record::Redirect]) {
        use crate::record::Direction;
        for target in targets {
            if target.direction == Direction::Out {
                let channel = if target.duplicate {
                    target
                        .target
                        .trim_end_matches('-')
                        .parse::<i32>()
                        .ok()
                        .and_then(|fd| scope.output_fds.get(&fd).copied().flatten())
                } else {
                    None
                };
                Rc::make_mut(&mut scope.output_fds).insert(target.fd, channel);
                if target.duplicate
                    && target.target.ends_with('-')
                    && let Ok(source) = target.target.trim_end_matches('-').parse::<i32>()
                {
                    Rc::make_mut(&mut scope.output_fds).insert(source, None);
                }
            } else if matches!(
                target.direction,
                Direction::In | Direction::Heredoc | Direction::Herestring
            ) {
                let source = target
                    .duplicate
                    .then(|| target.target.trim_end_matches('-').parse::<i32>().ok())
                    .flatten();
                let id = source
                    .and_then(|fd| {
                        if fd == 0 {
                            Some(scope.stdin_id)
                        } else {
                            scope.input_fds.get(&fd).copied()
                        }
                    })
                    .unwrap_or_else(|| self.flow.channel());
                let input = if target.duplicate {
                    source
                        .and_then(|fd| scope.input(fd).cloned())
                        .unwrap_or_else(|| self.flow.unknown())
                } else if let Some(stream) = target.stream.as_deref() {
                    stream.value.clone()
                } else if matches!(target.direction, Direction::Heredoc | Direction::Herestring)
                    && !target.expands
                {
                    self.flow.bytes(
                        format!(
                            "{}{}",
                            target.target,
                            if target.direction == Direction::Herestring {
                                "\n"
                            } else {
                                ""
                            }
                        ),
                        scope.flow_guard.as_ref().clone(),
                    )
                } else if target.target == "/dev/null" && !target.expands {
                    Flow::default()
                } else {
                    self.flow.unknown()
                };
                Rc::make_mut(&mut scope.input_cursors).insert(id, input.clone());
                if target.fd == 0 {
                    scope.stdin_id = id;
                    scope.pipeline_input = Some(input);
                } else {
                    Rc::make_mut(&mut scope.input_fds).insert(target.fd, id);
                }
                if target.duplicate
                    && target.target.ends_with('-')
                    && let Some(source) = source
                {
                    let closed = self.flow.channel();
                    let unknown = self.flow.unknown();
                    Rc::make_mut(&mut scope.input_cursors).insert(closed, unknown.clone());
                    if source == 0 {
                        scope.stdin_id = closed;
                        scope.pipeline_input = Some(unknown);
                    } else {
                        Rc::make_mut(&mut scope.input_fds).insert(source, closed);
                    }
                }
            }
        }
    }

    pub(super) fn guarded_output(&mut self, scope: &Scope, output: &Output) -> Output {
        let channels = output
            .keys()
            .copied()
            .chain(scope.output_fds.values().filter_map(|channel| *channel))
            .collect::<BTreeSet<_>>();
        channels
            .into_iter()
            .map(|channel| {
                let value = output.get(&channel).cloned().unwrap_or_default();
                let guard = self
                    .flow
                    .bytes(String::new(), scope.flow_guard.as_ref().clone());
                (channel, self.flow.sequence(vec![guard, value]))
            })
            .collect()
    }

    pub(super) fn command_output(&mut self, command: &Command, scope: &Scope) -> Output {
        if scope.flow_end {
            return Output::new();
        }
        let Some(channel) = scope.output_fds.get(&1).copied().flatten() else {
            return Output::new();
        };
        let Some(index) = command.program else {
            if command.wrappers.iter().any(|wrapper| wrapper == "xargs") {
                let unknown = self.flow.unknown();
                let value = scope.pipeline_input.as_ref().map_or_else(
                    || unknown.clone(),
                    |input| self.flow.choice(vec![input.clone(), unknown.clone()]),
                );
                return Output::from([(channel, value)]);
            }
            return Output::new();
        };
        let args = &command.argv[index + 1..];
        let name = if command.wrappers.iter().any(|wrapper| wrapper == "xargs") {
            ""
        } else {
            command.argv[index].rsplit('/').next().unwrap_or("")
        };
        let mut guards = vec![scope.flow_guard.as_ref().clone()];
        for word in &command.argv {
            let right = scope.word_guards(word, &mut self.flow);
            guards = self.flow.guards(&guards, &right);
        }
        let mut candidates = Vec::new();
        for guard in guards {
            let flow = match name {
                "printf" | "echo" => self.printed_output(name, args, &guard),
                "cat" | "tee" => self.copied_output(name, args, scope),
                _ => {
                    let unknown = self.flow.unknown();
                    if let Some(input) = &scope.pipeline_input {
                        self.flow.choice(vec![input.clone(), unknown])
                    } else {
                        unknown
                    }
                }
            };
            let tag = self.flow.bytes(String::new(), guard);
            candidates.push(self.flow.sequence(vec![tag, flow]));
        }
        Output::from([(channel, self.flow.choice(candidates))])
    }

    fn printed_output(&mut self, name: &str, args: &[crate::record::Word], guard: &Guard) -> Flow {
        if name == "printf" && args.first().is_some_and(|word| word == "-v") {
            Flow::default()
        } else if args
            .first()
            .is_some_and(|word| word.expands || word.runtime_unknown || word.globs)
        {
            self.flow.unknown()
        } else {
            let unknown = args.iter().any(|word| {
                word.expands || word.runtime_unknown || word.globs || word.cardinality_unknown
            });
            let args = args
                .iter()
                .map(|word| {
                    if word.expands || word.runtime_unknown || word.globs {
                        crate::record::Word::literal(String::new())
                    } else {
                        word.clone()
                    }
                })
                .collect::<Vec<_>>();
            let texts = if name == "printf" {
                shell::pipeline::printf_output(&args).map(|text| vec![text])
            } else {
                let (args, newline) = if args.first().is_some_and(|word| word == "-n") {
                    (&args[1..], "")
                } else {
                    (args.as_slice(), "\n")
                };
                if args.first().is_some_and(|word| word.starts_with('-')) {
                    None
                } else {
                    let text = args
                        .iter()
                        .map(crate::record::Word::as_str)
                        .collect::<Vec<_>>()
                        .join(" ");
                    Some(vec![
                        format!("{text}{newline}"),
                        format!("{}{newline}", shell::pipeline::unescape(&text, true)),
                    ])
                }
            };
            if let Some(texts) = texts {
                let parts = texts
                    .into_iter()
                    .map(|text| self.flow.bytes(text, guard.clone()))
                    .collect();
                let known = self.flow.choice(parts);
                if unknown {
                    let unknown = self.flow.unknown();
                    self.flow.sequence(vec![known, unknown])
                } else {
                    known
                }
            } else {
                self.flow.unknown()
            }
        }
    }

    fn copied_output(&mut self, name: &str, args: &[crate::record::Word], scope: &Scope) -> Flow {
        if args
            .iter()
            .any(|word| word.starts_with('-') && word != "--" && word != "-")
        {
            self.flow.unknown()
        } else if name == "tee" {
            scope
                .pipeline_input
                .clone()
                .unwrap_or_else(|| self.flow.unknown())
        } else {
            let operands = args
                .iter()
                .filter(|word| word.as_str() != "--")
                .collect::<Vec<_>>();
            let mut parts = Vec::new();
            if operands.is_empty() {
                parts.push(
                    scope
                        .pipeline_input
                        .clone()
                        .unwrap_or_else(|| self.flow.unknown()),
                );
            }
            for word in operands {
                parts.push(if let Some(stream) = word.stream.as_deref() {
                    stream.value.clone()
                } else if word == "-" {
                    scope
                        .pipeline_input
                        .clone()
                        .unwrap_or_else(|| self.flow.unknown())
                } else {
                    self.flow.unknown()
                });
            }
            self.flow.sequence(parts)
        }
    }
}
