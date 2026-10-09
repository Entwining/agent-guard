use super::super as shell;
use super::*;

/// Shell state of one `|` operator, for the stages before it and the stage after it.
struct Pipe {
    inherited_input: Option<Flow>,
    before: Scope,
    start: usize,
    channel: usize,
    producer: Scope,
}

impl<'a, 'b> Evaluator<'a, 'b> {
    /// Evaluates stages as the left-nested `(a | b) | c` they denote: each pipe's
    /// producer scope encloses the pipes before it. Keeping the pipes in a list
    /// stops stack depth from growing with the stage count.
    pub(super) fn pipeline_statement(
        &mut self,
        stages: &[Statement],
        scope: &mut Scope,
        context: (usize, usize, bool),
    ) -> Result<Output, CheckError> {
        let (depth, source_id, nested) = context;
        let mut pipes = Vec::<Pipe>::with_capacity(stages.len());
        for _ in 1..stages.len() {
            let outer = pipes.last().map_or(&*scope, |pipe| &pipe.producer);
            let inherited_input = outer.pipeline_input.clone();
            let before = outer.isolated();
            let channel = self.flow.channel();
            let mut producer = before.isolated();
            producer.pipeline_input = inherited_input.clone();
            producer.capture(channel);
            producer.piped = true;
            pipes.push(Pipe {
                inherited_input,
                before,
                start: self.output.script.commands.len(),
                channel,
                producer,
            });
        }
        let Some(first) = stages.first() else {
            return Ok(Output::new());
        };
        let producer = pipes
            .last_mut()
            .map_or(&mut *scope, |pipe| &mut pipe.producer);
        let mut left_output = self.statement(first, producer, depth, source_id, nested)?;
        while let Some(pipe) = pipes.pop() {
            crate::check_deadline(self.deadline)?;
            // Later stages rescan every earlier command of this pipeline.
            if self.output.script.commands.len() > 512 {
                self.output.gap(CoverageGap::InspectionBudget);
                return Ok(Output::new());
            }
            let stage = &stages[stages.len() - 1 - pipes.len()];
            let middle = self.output.script.commands.len();
            let mut rhs = pipe.before.isolated();
            rhs.piped = true;
            rhs.stdin_id = pipe.channel;
            let input = left_output.remove(&pipe.channel).unwrap_or_default();
            rhs.pipeline_input = Some(input.clone());
            Rc::make_mut(&mut rhs.input_cursors).insert(pipe.channel, input);
            let right_output = self.statement(stage, &mut rhs, depth, source_id, nested)?;
            let output = self.flow.outputs(&[left_output, right_output], false);
            let (left, right) =
                self.output.script.commands[pipe.start..].split_at_mut(middle - pipe.start);
            shell::pipeline::mark_walked_input(left, right);
            let sources = shell::pipeline::xargs_replacements(left, right);
            for input in sources {
                self.source(
                    &input.source,
                    &mut Scope::new(self.frontend.host.home, &input.cwd),
                    depth + 1,
                )?;
            }
            let outer = pipes
                .last_mut()
                .map_or(&mut *scope, |pipe| &mut pipe.producer);
            let lhs = outer.clone();
            self.merge_bindings(outer, &[lhs, rhs.clone()]);
            self.merge_directories(outer, &[rhs]);
            outer.pipeline_input = pipe.inherited_input;
            left_output = if pipes.is_empty() {
                output
            } else {
                // An inner pipe reached its consumer as a compound statement.
                let active = self.flow.outputs(&[output], false);
                self.flow.outputs(&[active], true)
            };
        }
        Ok(left_output)
    }
}
