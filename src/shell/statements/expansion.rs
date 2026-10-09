use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub fn expand(
        &mut self,
        raw: &RawWord,
        scope: &mut Scope,
        depth: usize,
    ) -> Result<Vec<Expanded>, CheckError> {
        shell::expand_scoped(raw, scope, self, depth)
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
        for code in shell::arithmetic::evaluate(expression)?.code {
            self.isolated_source(&code, scope, depth + 1)?;
        }
        Ok(())
    }
}
