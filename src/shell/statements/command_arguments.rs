use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn command_arguments(
        &mut self,
        argv: &[RawWord],
        scope: &mut Scope,
        depth: usize,
    ) -> Result<Vec<Vec<Vec<crate::record::Word>>>, CheckError> {
        let mut arguments = Vec::new();
        for raw in argv {
            let declaration = argv.first().is_some_and(|w| {
                matches!(w.raw.as_str(), "export" | "local" | "declare" | "typeset")
            });
            let mut choices = Vec::new();
            if declaration && let Some((name, value)) = assignment(&raw.raw) {
                for expanded in self.expand(
                    &RawWord {
                        raw: value.into(),
                        syntax: WordSyntax::Shell,
                        expansions: raw.expansions.clone(),
                    },
                    scope,
                    depth,
                )? {
                    let mut word = expanded.word;
                    word.text = format!("{name}={}", word.text);
                    for range in word.cwd_ranges.iter_mut().chain(&mut word.quoted_ranges) {
                        range.start += name.len() + 1;
                        range.end += name.len() + 1;
                    }
                    word.value = word.text.clone();
                    word.raw = raw.raw.clone();
                    choices.push(vec![word]);
                }
            } else {
                for expanded in self.expand(raw, scope, depth)? {
                    choices.push(expanded.split);
                    if !expanded.positional {
                        choices.push(vec![expanded.word]);
                    }
                }
            }
            choices.dedup();
            arguments.push(choices);
        }
        Ok(arguments)
    }

    fn independent_arguments(
        &self,
        prefixes: &[crate::record::Word],
        arguments: &[Vec<Vec<crate::record::Word>>],
        initial: &mut [crate::record::Word],
        scope: &Scope,
        context: (Option<usize>, bool),
    ) -> bool {
        let resolution = shell::argv::resolve(
            initial,
            &scope.directory.current.render(),
            self.frontend.host,
        );
        prefixes.is_empty()
            && !scope.piped
            && context.0.is_none()
            && resolution.program == Some(0)
            && resolution.wrappers.is_empty()
            && !initial[0].expands
            && !self.functions.contains_key(&initial[0].text)
            && arguments.first().is_some_and(|choices| choices.len() == 1)
            && arguments.iter().all(|choices| {
                !choices.is_empty()
                    && choices
                        .iter()
                        .all(|choice| choice.len() == 1 && !choice[0].starts_with('-'))
            })
            && crate::targets::infer(
                &Command {
                    argv: initial.to_vec(),
                    program: resolution.program,
                    cwd: resolution.cwd,
                    wrappers: resolution.wrappers,
                    shell: resolution.shell,
                    function: false,
                    redirects: Vec::new(),
                    flags: Vec::new(),
                    items: None,
                    stdin: Stdin::None,
                    pipeline: None,
                    nested: context.1,
                },
                &scope.directory.current.render(),
                self.frontend.host,
            )
            .independent_arguments
    }

    pub(super) fn command_alternatives(
        &mut self,
        prefixes: Vec<crate::record::Word>,
        arguments: &[Vec<Vec<crate::record::Word>>],
        scope: &Scope,
        context: (Option<usize>, bool),
    ) -> Vec<ArgumentAlternative> {
        let mut initial = prefixes.clone();
        for choices in arguments {
            if let Some(choice) = choices.first() {
                initial.extend(choice.clone());
            }
        }
        let independent =
            self.independent_arguments(&prefixes, arguments, &mut initial, scope, context);
        let mut alternatives = if independent {
            vec![ArgumentAlternative::new(initial.clone())]
        } else {
            vec![ArgumentAlternative::new(prefixes)]
        };
        for (index, choices) in arguments.iter().enumerate() {
            if independent {
                // Every candidate still reaches its owner; independent fields
                // need their union, not every Cartesian combination.
                for choice in choices.iter().skip(1) {
                    let mut candidate = initial.clone();
                    candidate[index] = choice[0].clone();
                    alternatives.push(ArgumentAlternative::new(candidate));
                }
                continue;
            }
            let mut next = Vec::new();
            for mut previous in alternatives {
                let remaining = 512 - next.len();
                let mut viable = Vec::new();
                for choice in choices {
                    #[cfg(test)]
                    {
                        self.output.candidate_pairs += 1;
                    }
                    if !previous.compatible(
                        choice,
                        #[cfg(test)]
                        &mut self.output.argument_compatibility_checks,
                    ) {
                        continue;
                    }
                    if viable.len() == remaining {
                        self.output.gap(CoverageGap::InspectionBudget);
                        break;
                    }
                    viable.push(choice);
                }
                if let [choice] = viable.as_slice() {
                    previous.extend(
                        choice,
                        #[cfg(test)]
                        &mut self.output.argv_prefix_word_copies,
                    );
                    next.push(previous);
                } else {
                    for choice in viable {
                        let mut candidate = previous.clone();
                        candidate.extend(
                            choice,
                            #[cfg(test)]
                            &mut self.output.argv_prefix_word_copies,
                        );
                        next.push(candidate);
                    }
                }
            }
            alternatives = next;
        }
        alternatives
    }
}
