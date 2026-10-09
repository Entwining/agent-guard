use super::super as shell;
use super::*;

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn loop_iteration(
        &mut self,
        body: &[Statement],
        scope: &mut Scope,
        outputs: &mut Vec<Output>,
        stopped: &mut Vec<Output>,
        context: (usize, usize, bool),
    ) -> Result<(), CheckError> {
        let before = scope.loops.last().map_or(0, |states| states.len());
        let (depth, source_id, nested) = context;
        let output = self.run(body, scope, depth + 1, source_id, nested)?;
        outputs.push(output);
        let states = scope
            .loops
            .last()
            .into_iter()
            .flat_map(|states| states.iter().skip(before))
            .cloned()
            .collect::<Vec<_>>();
        if !scope.channels.is_empty() && states.iter().any(|state| !state.continue_loop) {
            let prefix = self.flow.outputs(&std::mem::take(outputs), false);
            let mut active = prefix.clone();
            for state in states.iter().filter(|state| !state.continue_loop) {
                stopped.push(self.flow.filter_output(&prefix, &state.flow_guard, false));
                active = self.flow.filter_output(&active, &state.flow_guard, true);
            }
            outputs.push(active);
        }
        let mut continuing = states
            .into_iter()
            .filter(|state| state.continue_loop)
            .map(|state| scope.with_state(state))
            .collect::<Vec<_>>();
        if !continuing.is_empty() {
            if !scope.flow_end {
                continuing.push(scope.clone());
            }
            self.merge_bindings(scope, &continuing);
            scope.flow_end = false;
        }
        Ok(())
    }

    pub(super) fn compound_statement(
        &mut self,
        statement: &Statement,
        scope: &mut Scope,
        depth: usize,
        source_id: usize,
        nested: bool,
    ) -> Result<Output, CheckError> {
        let mut outputs = Vec::new();
        let mut stopped = Vec::new();
        match statement {
            Statement::Redirected(redirects, body) => {
                outputs.push(self.redirected_statement(
                    redirects,
                    body,
                    scope,
                    (depth, source_id, nested),
                )?);
            }
            Statement::ArrayAssignment {
                name,
                values,
                append,
                declaration,
            } => {
                self.array_assignment(name, values, *append, *declaration, scope, depth)?;
            }
            Statement::UnsupportedSyntax => self.output.gap(CoverageGap::UnsupportedShellSyntax),
            Statement::Group(body) => {
                outputs.push(self.run(body, scope, depth + 1, source_id, nested)?)
            }
            Statement::Subshell(body) | Statement::Async(body) | Statement::Substitution(body) => {
                outputs.push(self.isolated_statement(
                    statement,
                    body,
                    scope,
                    (depth, source_id, nested),
                )?);
            }
            Statement::Definition(name, body) => {
                self.definition_statement(name, body, scope, (depth, source_id, nested))?;
            }
            Statement::Binary(Operator::And | Operator::Or, _, _) => {
                outputs.push(self.logical_list(statement, scope, depth, source_id, nested)?);
            }
            Statement::Binary(Operator::Pipe, left, right) => {
                outputs.push(self.pipe_statement(
                    left,
                    right,
                    scope,
                    (depth, source_id, nested),
                )?);
            }
            Statement::Conditional {
                condition,
                then,
                otherwise,
            } => {
                outputs.push(self.conditional_statement(
                    condition,
                    then,
                    otherwise,
                    scope,
                    (depth, source_id, nested),
                )?);
            }
            Statement::Case {
                words,
                branches,
                exhaustive,
            } => {
                outputs.push(self.case_statement(
                    words,
                    branches,
                    *exhaustive,
                    scope,
                    (depth, source_id, nested),
                )?);
            }
            Statement::Loop {
                variable,
                header,
                body,
                empty,
            } => {
                return self.loop_statement(
                    variable.as_deref(),
                    header,
                    body,
                    *empty,
                    scope,
                    (depth, source_id, nested),
                );
            }
            Statement::Use(word) => self.word_use(word, scope, depth, nested)?,
            Statement::Expansion(word) => {
                self.expand(word, scope, depth)?;
            }
            Statement::Command { .. } => {
                outputs.push(self.statement(statement, scope, depth, source_id, nested)?);
            }
        }
        let active = self.flow.outputs(&outputs, false);
        stopped.push(active);
        Ok(self.flow.outputs(&stopped, true))
    }

    fn redirected_statement(
        &mut self,
        redirects: &[shell::RawRedirect],
        body: &[Statement],
        scope: &mut Scope,
        context: (usize, usize, bool),
    ) -> Result<Output, CheckError> {
        let (depth, source_id, nested) = context;

        let prior = scope.pipeline_input.clone();
        let prior_inputs = scope.input_fds.clone();
        let prior_stdin = scope.stdin_id;
        let prior_outputs = scope.output_fds.clone();
        let targets = self.expanded_redirects(redirects, scope, depth)?;
        self.redirect_fds(scope, &targets);
        self.emit(
            Command {
                function: false,
                environment: Vec::new(),
                argv: Vec::new(),
                redirects: targets,
                cwd: scope.directory.current.render(),
                program: None,
                wrappers: Vec::new(),
                shell: true,
                flags: Vec::new(),
                items: None,
                stdin: Stdin::None,
                pipeline: None,
                nested,
            },
            scope,
            &[],
        );
        let output = self.run(body, scope, depth + 1, source_id, nested)?;
        scope.restore_inputs(prior_stdin, prior, prior_inputs);
        scope.output_fds = prior_outputs;

        Ok(output)
    }

    fn isolated_statement(
        &mut self,
        statement: &Statement,
        body: &[Statement],
        scope: &mut Scope,
        context: (usize, usize, bool),
    ) -> Result<Output, CheckError> {
        let (depth, source_id, nested) = context;

        let functions = self.functions.clone();
        let mut child = scope.isolated();
        if matches!(statement, Statement::Subshell(_)) {
            child.pipeline_input = scope.pipeline_input.clone();
        }
        let result = self.run(
            body,
            &mut child,
            depth + 1,
            source_id,
            nested || matches!(statement, Statement::Substitution(_)),
        );
        self.functions = functions;
        let output = result?;
        if !matches!(statement, Statement::Async(_)) {
            scope.inherit_input_progress(&child);
        }

        Ok(output)
    }

    fn definition_statement(
        &mut self,
        name: &str,
        body: &[Statement],
        scope: &mut Scope,
        context: (usize, usize, bool),
    ) -> Result<(), CheckError> {
        let (depth, source_id, nested) = context;

        if scope.isolated || scope.conditional_definition {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
        }
        let exported = self
            .functions
            .get(name)
            .is_some_and(|function| function.exported);
        if Rc::make_mut(&mut self.functions)
            .insert(
                name.to_owned(),
                Function {
                    body: Rc::new(body.to_vec()),
                    source_id,
                    exported,
                },
            )
            .is_some()
            && scope.frames.is_empty()
        {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
        }
        let mut inner = scope.isolated();
        // Body inspection must not publish nested definitions or create
        // an isolated-shell refusal for an ordinary function frame.
        inner.isolated = scope.isolated;
        inner.defining = true;
        inner.channels = Rc::default();
        inner.output_fds = Rc::default();
        inner.enter_function();
        // Definitions have unknown argv, not an invocation with no arguments.
        self.positionals(&[], &mut inner);
        inner.assign("#".into(), vec![BindingValue::RuntimeUnknown(None)]);
        Rc::make_mut(&mut inner.bindings).remove("@");
        let inserted = self.running.insert(name.to_owned());
        let functions = self.functions.clone();
        let result = self.run(body, &mut inner, depth + 1, source_id, nested);
        self.functions = functions;
        if inserted {
            self.running.remove(name);
        }
        result?;

        Ok(())
    }

    fn pipe_statement(
        &mut self,
        left: &Statement,
        right: &Statement,
        scope: &mut Scope,
        context: (usize, usize, bool),
    ) -> Result<Output, CheckError> {
        let (depth, source_id, nested) = context;

        let inherited_input = scope.pipeline_input.clone();
        let before = scope.isolated();
        let start = self.output.script.commands.len();
        let channel = self.flow.channel();
        let mut producer = before.isolated();
        producer.pipeline_input = inherited_input.clone();
        producer.capture(channel);
        producer.piped = true;
        let mut left_output = self.statement(left, &mut producer, depth, source_id, nested)?;
        let middle = self.output.script.commands.len();
        let mut rhs = before.isolated();
        rhs.piped = true;
        rhs.stdin_id = channel;
        let input = left_output.remove(&channel).unwrap_or_default();
        rhs.pipeline_input = Some(input.clone());
        Rc::make_mut(&mut rhs.input_cursors).insert(channel, input);
        let right_output = self.statement(right, &mut rhs, depth, source_id, nested)?;
        let output = self.flow.outputs(&[left_output, right_output], false);
        let (left, right) = self.output.script.commands[start..].split_at_mut(middle - start);
        shell::pipeline::mark_walked_input(left, right);
        let sources = shell::pipeline::xargs_replacements(left, right);

        for input in sources {
            self.source(
                &input.source,
                &mut Scope::new(self.frontend.host.home, &input.cwd),
                depth + 1,
            )?;
        }
        let lhs = scope.clone();
        self.merge_bindings(scope, &[lhs, rhs.clone()]);
        self.merge_directories(scope, &[rhs]);
        scope.pipeline_input = inherited_input;

        Ok(output)
    }

    fn conditional_statement(
        &mut self,
        condition: &[Statement],
        then: &[Statement],
        otherwise: &[Statement],
        scope: &mut Scope,
        context: (usize, usize, bool),
    ) -> Result<Output, CheckError> {
        let (depth, source_id, nested) = context;

        let mut outputs = Vec::new();
        let before = scope.clone();
        let mut test = scope.clone();
        test.directory.failures = None;
        outputs.push(self.run(condition, &mut test, depth + 1, source_id, nested)?);
        let choice = self.flow.choice_id();
        let mut yes = test.branch();
        yes.conditional_append = true;
        Rc::make_mut(&mut yes.flow_guard).insert(choice, 0);
        let yes_output = self.run(then, &mut yes, depth + 1, source_id, nested)?;
        let yes_output = self.guarded_output(&yes, &yes_output);
        let mut no = test.branch();
        no.conditional_append = true;
        Rc::make_mut(&mut no.flow_guard).insert(choice, 1);
        let no_output = self.run(otherwise, &mut no, depth + 1, source_id, nested)?;
        let no_output = self.guarded_output(&no, &no_output);
        let (yes_output, no_output) = if yes.same_flow_state(&no) {
            yes.forget_choice(choice);
            no.forget_choice(choice);
            (
                self.flow.forget_output(yes_output, choice)?,
                self.flow.forget_output(no_output, choice)?,
            )
        } else {
            (yes_output, no_output)
        };
        let output = self.flow.outputs(&[yes_output, no_output], true);
        self.merge_directories(scope, &[before, test.clone(), yes.clone(), no.clone()]);
        let branches = vec![yes, no];
        self.merge_bindings(scope, &branches);

        outputs.push(output);
        Ok(self.flow.outputs(&outputs, false))
    }

    fn case_statement(
        &mut self,
        words: &[RawWord],
        branches: &[Vec<Statement>],
        exhaustive: bool,
        scope: &mut Scope,
        context: (usize, usize, bool),
    ) -> Result<Output, CheckError> {
        let (depth, source_id, nested) = context;

        for word in words {
            self.word_use(word, scope, depth, nested)?;
        }
        let mut exits = Vec::new();
        let mut branch_outputs = Vec::new();
        let choice = self.flow.choice_id();
        if !exhaustive {
            let mut empty = scope.clone();
            Rc::make_mut(&mut empty.flow_guard).insert(choice, branches.len());
            branch_outputs.push(self.guarded_output(&empty, &Output::new()));
            exits.push(empty);
        }
        for (index, body) in branches.iter().enumerate() {
            let mut inner = scope.branch();
            inner.conditional_append = true;
            Rc::make_mut(&mut inner.flow_guard).insert(choice, index);
            let output = self.run(body, &mut inner, depth + 1, source_id, nested)?;
            branch_outputs.push(self.guarded_output(&inner, &output));
            exits.push(inner);
        }
        if exits
            .first()
            .is_some_and(|first| exits.iter().all(|branch| first.same_flow_state(branch)))
        {
            for branch in &mut exits {
                branch.forget_choice(choice);
            }
            branch_outputs = branch_outputs
                .into_iter()
                .map(|output| self.flow.forget_output(output, choice))
                .collect::<Result<Vec<_>, _>>()?;
        }
        let output = self.flow.outputs(&branch_outputs, true);
        self.merge_directories(scope, &exits);
        self.merge_bindings(scope, &exits);

        Ok(output)
    }
}
