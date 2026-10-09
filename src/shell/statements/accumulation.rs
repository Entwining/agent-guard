use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn literal_accumulation(
        &mut self,
        name: &str,
        raw: &RawWord,
        scope: &mut Scope,
        depth: usize,
    ) -> Result<Option<Vec<BindingValue>>, CheckError> {
        if !scope.bounded_loop
            || !(scope.conditional_append
                || scope.bindings.get(name).is_some_and(|binding| {
                    binding
                        .values
                        .iter()
                        .any(|value| matches!(value, BindingValue::RepeatedFields(_)))
                }))
            || name.ends_with('+')
            || scope
                .bindings
                .get(name)
                .is_some_and(|binding| binding.arithmetic)
        {
            return Ok(None);
        }
        let Some(tail) = shell::words::without_leading_parameter(&raw.raw, name) else {
            return Ok(None);
        };
        let variables = scope.contexts();
        let cwd = scope.directory.current.render();
        let preview = shell::words::expand(
            &tail,
            &raw.syntax,
            &shell::words::ExpansionContext {
                zsh: false,
                assignments: &[],
                variables: &variables,
                unknown_variables: &scope.unknown_parameters(&variables),
                deadline: self.deadline,
                runtime_variables: &BTreeSet::new(),
                pattern_variables: &scope.pattern_contexts(&variables),
                host: self.frontend.host,
                cwd: &cwd,
                tilde_assigned: true,
            },
        )?;
        if preview.word.expands
            || preview.word.globs
            || preview.unsupported
            || !preview.nested.is_empty()
            || !preview.arithmetic.is_empty()
            || preview.word.vars.iter().any(|variable| {
                variable == name
                    || scope.bindings.get(variable).is_some_and(|binding| {
                        binding.values.iter().any(|value| value.known().is_none())
                    })
            })
        {
            return Ok(None);
        }
        let Some(mut repeated) = literal_repetitions(scope, name) else {
            return Ok(None);
        };
        let tails = shell::expand_scoped(
            &RawWord {
                raw: tail,
                syntax: raw.syntax.clone(),
                expansions: Vec::new(),
            },
            scope,
            self,
            depth,
        )?;
        for tail in tails {
            let tail = tail.word.text;
            // Field boundaries keep every literal's resource identity independent
            // of repetition count. Contiguous bytes keep the sequential owner.
            if !tail.starts_with([' ', '\t', '\n']) {
                return Ok(None);
            }
            for repetition in &mut repeated {
                if !repetition.alternatives.contains(&tail) {
                    if repetition.alternatives.len() == 512 {
                        return Ok(None);
                    }
                    repetition.alternatives.push(tail.clone());
                }
            }
        }
        Ok(Some(
            repeated
                .into_iter()
                .map(|value| BindingValue::RepeatedFields(Box::new(value)))
                .collect(),
        ))
    }
}

fn literal_repetitions(scope: &Scope, name: &str) -> Option<Vec<LiteralRepetition>> {
    let prior = scope.bindings.get(name).map_or_else(
        || vec![BindingValue::Known(String::new())],
        |binding| binding.values.as_ref().clone(),
    );
    let mut repeated = Vec::new();
    for value in prior {
        let value = match value {
            BindingValue::Known(prefix) => LiteralRepetition {
                prefix,
                alternatives: Vec::new(),
                suffix: String::new(),
                may_be_empty: false,
            },
            BindingValue::RepeatedFields(mut value) if value.suffix.is_empty() => {
                value.may_be_empty = false;
                *value
            }
            _ => return None,
        };
        repeated.push(value);
    }
    Some(repeated)
}
