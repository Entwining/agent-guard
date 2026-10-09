use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub fn expand(
        &mut self,
        raw: &RawWord,
        scope: &mut Scope,
        depth: usize,
    ) -> Result<Vec<Expanded>, CheckError> {
        shell::expand_scoped(raw, scope, self, depth, true)
    }
    pub fn isolated_source(
        &mut self,
        source: &str,
        scope: &mut Scope,
        depth: usize,
    ) -> Result<(), CheckError> {
        let functions = self.functions.clone();
        let mut child = scope.isolated();
        let result = self.source(source, &mut child, depth);
        self.functions = functions;
        result?;
        Ok(())
    }
    pub fn arithmetic(
        &mut self,
        expression: &str,
        scope: &mut Scope,
        depth: usize,
    ) -> Result<(), CheckError> {
        self.armed_references(expression, scope, depth)?;
        let code = self.arithmetic_code(expression, scope)?;
        for code in code {
            self.isolated_source(&code, scope, depth + 1)?;
        }
        Ok(())
    }
    pub fn arithmetic_code(
        &mut self,
        expression: &str,
        scope: &Scope,
    ) -> Result<Vec<String>, CheckError> {
        let evaluation = shell::arithmetic::evaluate(expression, &scope.values())?;
        if evaluation.bounded {
            self.output.gap(CoverageGap::InspectionBudget);
        }
        if evaluation.names.iter().any(|name| {
            scope.bindings.get(name).is_some_and(|b| {
                b.values.iter().any(|value| {
                    matches!(
                        value,
                        BindingValue::RepeatedFields(_) | BindingValue::Undetermined
                    )
                })
            })
        }) {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
        }
        Ok(evaluation.code)
    }
    pub(super) fn armed_word(
        &mut self,
        word: &crate::record::Word,
        scope: &mut Scope,
        depth: usize,
    ) -> Result<(), CheckError> {
        if identifier(&word.text) {
            return self.armed_reference(&word.text, scope, depth);
        }
        if let Some((name, value)) = word.text.split_once('=') {
            let name = name.strip_suffix('+').unwrap_or(name);
            if identifier(name) || indexed_name(name).is_some() {
                self.armed_references(value, scope, depth)?;
                if let Some(index) = indexed_name(name) {
                    self.armed_references(index, scope, depth)?;
                }
            }
        } else if word.vars.is_empty()
            && let Some(index) = indexed_name(&word.text)
        {
            self.armed_references(index, scope, depth)?;
        }
        Ok(())
    }
    pub fn armed_references(
        &mut self,
        expression: &str,
        scope: &mut Scope,
        depth: usize,
    ) -> Result<(), CheckError> {
        let names = match shell::arithmetic::evaluate(expression, &BTreeMap::new()) {
            Ok(evaluation) => evaluation.names,
            Err(error) if error.kind == crate::CheckErrorKind::ResourceLimit => {
                self.output.gap(CoverageGap::InspectionBudget);
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        for name in names {
            self.armed_reference(&name, scope, depth)?;
        }
        Ok(())
    }
    pub(in crate::shell) fn armed_reference(
        &mut self,
        name: &str,
        scope: &mut Scope,
        depth: usize,
    ) -> Result<(), CheckError> {
        let values = scope.named_candidates(name);
        if values.is_empty() {
            return Ok(());
        }
        let mut sources = Vec::new();
        for value in &values {
            let BindingValue::Known(value) = value else {
                if value == &BindingValue::Undetermined
                    || matches!(
                        value,
                        BindingValue::RuntimeDerived(_) | BindingValue::ShellDerived(_)
                    ) && value.lexical().is_some_and(|text| {
                        !matches!(
                            shell::arithmetic::armed(text),
                            shell::arithmetic::Arming::Inert
                        )
                    })
                {
                    self.output.gap(CoverageGap::UnsupportedShellSyntax);
                }
                continue;
            };
            match shell::arithmetic::armed(value) {
                shell::arithmetic::Arming::Armed(code) => {
                    for source in code {
                        if !sources.contains(&source) {
                            sources.push(source);
                        }
                    }
                }
                shell::arithmetic::Arming::Inert => {}
                shell::arithmetic::Arming::Unresolved => {
                    self.output.gap(CoverageGap::InspectionBudget);
                }
            }
        }
        if !sources.is_empty() {
            for source in self.arithmetic_code(name, scope)? {
                if !sources.contains(&source) {
                    sources.push(source);
                }
            }
            for source in sources {
                self.isolated_source(&source, scope, depth + 1)?;
            }
        }
        Ok(())
    }
}
