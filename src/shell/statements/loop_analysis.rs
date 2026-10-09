use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn loop_body_is_invariant(
        &mut self,
        body: &[Statement],
        scope: &Scope,
        variable: &str,
        ordered_output: bool,
    ) -> Result<bool, CheckError> {
        let mut inputs = BTreeSet::new();
        let mut writes = BTreeSet::new();
        let mut stable = BTreeSet::new();
        let local = if ordered_output {
            BTreeSet::new()
        } else {
            BTreeSet::from([variable.to_owned()])
        };
        // A body without a carried input needs one candidate-union analysis.
        // Stateful builtins, recursive functions, repeated loop names and dynamic writes
        // retain the sequential/convergence owner and its conservative limits.
        Ok(
            self.loop_inputs(body, scope, &local, &mut inputs, &mut writes, &mut stable)?
                && (!ordered_output || !inputs.contains(variable))
                && inputs
                    .iter()
                    .all(|name| !writes.contains(name) || stable.contains(name)),
        )
    }
    fn loop_word_inputs(
        &mut self,
        word: &RawWord,
        scope: &Scope,
        local: &BTreeSet<String>,
        inputs: &mut BTreeSet<String>,
        writes: &mut BTreeSet<String>,
        stable: &mut BTreeSet<String>,
    ) -> Result<Option<shell::Expanded>, CheckError> {
        let variables = scope.contexts();
        let cwd = scope.directory.current.render();
        let expanded = shell::words::expand(
            &word.raw,
            &word.syntax,
            &shell::words::ExpansionContext {
                named_dirs: &BTreeMap::new(),
                zsh: false,
                assignments: &[],
                variables: &variables,
                unknown_variables: &scope.unknown_parameters(&variables),
                deadline: self.deadline,
                runtime_variables: &BTreeSet::new(),
                pattern_variables: &scope.pattern_contexts(&variables),
                cwd: &cwd,
                host: self.frontend.host,
                tilde_assigned: true,
            },
        )?;
        if expanded.unsupported
            || !expanded.references.is_empty()
            || word
                .expansions
                .iter()
                .any(|e| matches!(e, shell::RawExpansion::Variable(_)))
        {
            return Ok(None);
        }
        for expression in &expanded.arithmetic {
            let evaluation = shell::arithmetic::evaluate(expression)?;
            if !evaluation.code.is_empty() {
                return Ok(None);
            }
            inputs.extend(
                evaluation
                    .names
                    .into_iter()
                    .filter(|name| !local.contains(name)),
            );
        }
        for name in &expanded.word.vars {
            if !local.contains(name) && !(local.contains("@") && name.parse::<usize>().is_ok()) {
                inputs.insert(name.clone());
            }
        }
        if identifier(&expanded.word.text) && !local.contains(&expanded.word.text) {
            inputs.insert(expanded.word.text.clone());
        }
        let mut sources = expanded.nested.clone();
        for expansion in &word.expansions {
            if let shell::RawExpansion::Code(source) = expansion
                && !sources.contains(source)
            {
                sources.push(source.clone());
            }
        }
        for source in sources {
            let parsed = self.parsed_source(&source)?;
            let Some(body) = &parsed.original else {
                return Ok(None);
            };
            if !self.loop_inputs(body, scope, local, inputs, writes, stable)? {
                return Ok(None);
            }
        }
        Ok(Some(expanded))
    }
    fn loop_inputs(
        &mut self,
        body: &[Statement],
        scope: &Scope,
        local: &BTreeSet<String>,
        inputs: &mut BTreeSet<String>,
        writes: &mut BTreeSet<String>,
        stable: &mut BTreeSet<String>,
    ) -> Result<bool, CheckError> {
        for statement in body {
            match statement {
                Statement::Group(body) => {
                    if !self.loop_inputs(body, scope, local, inputs, writes, stable)? {
                        return Ok(false);
                    }
                }
                Statement::Conditional {
                    condition,
                    then,
                    otherwise,
                } => {
                    for branch in [condition, then, otherwise] {
                        if !self.loop_inputs(branch, scope, local, inputs, writes, stable)? {
                            return Ok(false);
                        }
                    }
                }
                Statement::Binary(Operator::And | Operator::Or, left, right) => {
                    for branch in [left.as_ref(), right.as_ref()] {
                        if !self.loop_inputs(
                            std::slice::from_ref(branch),
                            scope,
                            local,
                            inputs,
                            writes,
                            stable,
                        )? {
                            return Ok(false);
                        }
                    }
                }
                Statement::Loop {
                    variable: Some(variable),
                    header,
                    body,
                    empty: false,
                } if !header.is_empty() && !local.contains(variable) => {
                    for word in header {
                        if self
                            .loop_word_inputs(word, scope, local, inputs, writes, stable)?
                            .is_none()
                        {
                            return Ok(false);
                        }
                    }
                    writes.insert(variable.clone());
                    let mut inner = local.clone();
                    inner.insert(variable.clone());
                    if !self.loop_inputs(body, scope, &inner, inputs, writes, stable)? {
                        return Ok(false);
                    }
                }
                Statement::Command {
                    assignments,
                    argv,
                    redirects,
                    pipeline: None,
                } => {
                    if !self.loop_command_inputs(
                        assignments,
                        argv,
                        redirects,
                        scope,
                        local,
                        (inputs, writes, stable),
                    )? {
                        return Ok(false);
                    }
                }
                _ => return Ok(false),
            }
        }
        Ok(true)
    }

    fn loop_command_inputs(
        &mut self,
        assignments: &[(String, RawWord)],
        argv: &[RawWord],
        redirects: &[shell::RawRedirect],
        scope: &Scope,
        local: &BTreeSet<String>,
        state: (
            &mut BTreeSet<String>,
            &mut BTreeSet<String>,
            &mut BTreeSet<String>,
        ),
    ) -> Result<bool, CheckError> {
        let (inputs, writes, stable) = state;
        if !assignments.is_empty() && !argv.is_empty() {
            return Ok(false);
        }
        if !self.loop_assignment_inputs(
            assignments,
            scope,
            local,
            (&mut *inputs, &mut *writes, &mut *stable),
        )? {
            return Ok(false);
        }
        let mut words = Vec::new();
        for word in argv {
            let Some(value) = self.loop_word_inputs(word, scope, local, inputs, writes, stable)?
            else {
                return Ok(false);
            };
            words.push(value.word);
        }
        let resolved = shell::argv::resolve(
            &mut words,
            &scope.directory.current.render(),
            self.frontend.host,
        );
        if resolved.source.is_some() {
            return Ok(false);
        }
        if let Some(index) = resolved.program {
            let program = &words[index];
            if program.expands
                || !program.vars.is_empty()
                || matches!(
                    program.rsplit('/').next(),
                    Some(
                        "cd" | "pushd"
                            | "popd"
                            | "read"
                            | "set"
                            | "unset"
                            | "export"
                            | "local"
                            | "declare"
                            | "typeset"
                            | "eval"
                            | "source"
                            | "."
                            | "break"
                            | "continue"
                            | "return"
                            | "let"
                    )
                )
                || program == "printf" && words.get(index + 1).is_some_and(|w| w == "-v")
            {
                return Ok(false);
            }
            if let Some(function) = self.functions.get(&program.text).cloned() {
                if self.running.len() >= MAX_NESTING || !self.running.insert(program.text.clone()) {
                    return Ok(false);
                }
                let mut function_local = local.clone();
                function_local.extend(["@".into(), "*".into(), "#".into()]);
                let invariant = self.loop_inputs(
                    &function.body,
                    scope,
                    &function_local,
                    inputs,
                    writes,
                    stable,
                );
                self.running.remove(&program.text);
                if !invariant? {
                    return Ok(false);
                }
            }
        }
        for redirect in redirects {
            if self
                .loop_word_inputs(&redirect.target, scope, local, inputs, writes, stable)?
                .is_none()
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn loop_assignment_inputs(
        &mut self,
        assignments: &[(String, RawWord)],
        scope: &Scope,
        local: &BTreeSet<String>,
        state: (
            &mut BTreeSet<String>,
            &mut BTreeSet<String>,
            &mut BTreeSet<String>,
        ),
    ) -> Result<bool, CheckError> {
        let (inputs, writes, stable) = state;
        for (name, word) in assignments {
            if !identifier(name) || local.contains(name) {
                return Ok(false);
            }
            let Some(value) = self.loop_word_inputs(word, scope, local, inputs, writes, stable)?
            else {
                return Ok(false);
            };
            if !value.word.vars.is_empty() {
                return Ok(false);
            }
            if value.word.expands && !value.nested.is_empty() {
                stable.insert(name.clone());
            } else if value.arithmetic.len() == 1 {
                let evaluation = shell::arithmetic::evaluate(&value.arithmetic[0])?;
                let self_update = scope.bindings.get(name).is_some_and(|binding| {
                    binding.values.iter().all(|candidate| match candidate {
                        BindingValue::Known(value) => value.parse::<i128>().is_ok(),
                        BindingValue::RuntimeUnknown(Some(value)) => value == &word.raw,
                        _ => false,
                    })
                });
                if evaluation.names.iter().any(|input| input != name) || !self_update {
                    return Ok(false);
                }
                stable.insert(name.clone());
            }
            writes.insert(name.clone());
        }
        Ok(true)
    }
}
