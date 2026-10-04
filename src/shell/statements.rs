use super::cwd::{self, Directory};
use super::{Expanded, Frontend, Observation, Operator, RawWord, Statement, WordSyntax};
use crate::{
    CheckError, CoverageGap,
    limits::MAX_NESTING,
    record::{Command, Role, Stdin},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Binding {
    pub values: Vec<Option<String>>,
}

#[derive(Clone)]
pub(super) struct Scope {
    pub directory: Directory,
    pub bindings: BTreeMap<String, Binding>,
    frames: Vec<BTreeMap<String, Option<Binding>>>,
    isolated: bool,
    defining: bool,
    conditional_definition: bool,
    returns: Vec<BindingState>,
    loops: Vec<Vec<BindingState>>,
}

#[derive(Clone, PartialEq, Eq)]
struct BindingState {
    bindings: BTreeMap<String, Binding>,
    frames: Vec<BTreeMap<String, Option<Binding>>>,
}

impl Scope {
    pub fn new(home: &str, cwd: &str) -> Self {
        Self {
            directory: Directory::new(cwd),
            bindings: BTreeMap::from([(
                "HOME".into(),
                Binding {
                    values: vec![Some(home.into())],
                },
            )]),
            frames: Vec::new(),
            isolated: false,
            defining: false,
            conditional_definition: false,
            returns: Vec::new(),
            loops: Vec::new(),
        }
    }
    pub fn isolated(&self) -> Self {
        let mut child = self.clone();
        child.returns.clear();
        child.loops.clear();
        child.isolated = true;
        child.directory.failures = None;
        child
    }
    fn branch(&self) -> Self {
        let mut child = self.clone();
        child.conditional_definition = true;
        child
    }
    fn state(&self) -> BindingState {
        BindingState {
            bindings: self.bindings.clone(),
            frames: self.frames.clone(),
        }
    }
    fn with_state(&self, state: BindingState) -> Self {
        let mut scope = self.clone();
        scope.bindings = state.bindings;
        scope.frames = state.frames;
        scope
    }
    pub fn values(&self) -> BTreeMap<String, Vec<String>> {
        self.bindings
            .iter()
            .map(|(name, b)| (name.clone(), b.values.iter().flatten().cloned().collect()))
            .collect()
    }
    pub fn contexts(&self) -> BTreeMap<String, String> {
        self.bindings
            .iter()
            .filter_map(|(n, b)| {
                b.values
                    .first()
                    .and_then(Option::as_ref)
                    .map(|v| (n.clone(), v.clone()))
            })
            .collect::<BTreeMap<_, _>>()
    }
    fn local(&mut self, name: &str) {
        if let Some(frame) = self.frames.last_mut() {
            frame
                .entry(name.into())
                .or_insert_with(|| self.bindings.get(name).cloned());
        }
    }
    fn assign(&mut self, name: String, values: Vec<Option<String>>) {
        self.bindings.insert(name, Binding { values });
    }
    fn enter_function(&mut self) {
        self.frames.push(BTreeMap::new());
    }
    fn leave_function(&mut self) {
        if let Some(frame) = self.frames.pop() {
            for (name, prior) in frame {
                if let Some(prior) = prior {
                    self.bindings.insert(name, prior);
                } else {
                    self.bindings.remove(&name);
                }
            }
        }
    }
    fn join(&mut self, branches: &[Scope]) -> bool {
        let keys = branches
            .iter()
            .flat_map(|s| s.bindings.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut bounded = false;
        self.bindings.clear();
        for name in keys {
            let (value, bound) = join_values(branches.iter().map(|s| s.bindings.get(&name)));
            bounded |= bound;
            if let Some(value) = value {
                self.bindings.insert(name, value);
            }
        }
        for (index, frame) in self.frames.iter_mut().enumerate() {
            let names = branches
                .iter()
                .flat_map(|s| s.frames[index].keys())
                .cloned()
                .collect::<BTreeSet<_>>();
            for name in names {
                let (value, bound) = join_values(branches.iter().map(|s| {
                    s.frames[index]
                        .get(&name)
                        .map_or_else(|| s.bindings.get(&name), Option::as_ref)
                }));
                bounded |= bound;
                frame.insert(name, value);
            }
        }
        for branch in branches {
            for state in &branch.returns {
                if !self.returns.contains(state) {
                    if self.returns.len() == 512 {
                        bounded = true;
                    } else {
                        self.returns.push(state.clone());
                    }
                }
            }
            for (outer, inner) in self.loops.iter_mut().zip(&branch.loops) {
                for state in inner {
                    if !outer.contains(state) {
                        if outer.len() == 512 {
                            bounded = true;
                        } else {
                            outer.push(state.clone());
                        }
                    }
                }
            }
        }
        bounded
    }
}

fn join_values<'a>(values: impl Iterator<Item = Option<&'a Binding>>) -> (Option<Binding>, bool) {
    let mut joined = Vec::new();
    let mut present = false;
    let mut bounded = false;
    for binding in values {
        present |= binding.is_some();
        let values = binding.map_or_else(|| vec![None], |b| b.values.clone());
        for value in values {
            if !joined.contains(&value) {
                if joined.len() == 512 {
                    bounded = true;
                } else {
                    joined.push(value);
                }
            }
        }
    }
    (present.then_some(Binding { values: joined }), bounded)
}

fn restore_prefix(
    bindings: &mut BTreeMap<String, Binding>,
    prior: &BTreeMap<String, Option<Binding>>,
) {
    for (name, binding) in prior {
        if let Some(binding) = binding {
            bindings.insert(name.clone(), binding.clone());
        } else {
            bindings.remove(name);
        }
    }
}

#[derive(Clone)]
struct Function {
    body: Vec<Statement>,
    source_id: usize,
}

pub(super) struct Evaluator<'a, 'b> {
    pub frontend: Frontend<'a>,
    pub output: &'b mut Observation,
    functions: BTreeMap<String, Function>,
    running: BTreeSet<String>,
    inspected: usize,
    function_runs: usize,
    unresolved_calls: Vec<(usize, bool)>,
}

impl<'a, 'b> Evaluator<'a, 'b> {
    pub fn new(frontend: Frontend<'a>, output: &'b mut Observation) -> Self {
        Self {
            frontend,
            output,
            functions: BTreeMap::new(),
            running: BTreeSet::new(),
            inspected: 0,
            function_runs: 0,
            unresolved_calls: Vec::new(),
        }
    }
    pub fn finish(&mut self) {
        for (index, defining) in &self.unresolved_calls {
            let command = &mut self.output.script.commands[*index];
            if let Some(program) = command.program
                && self.functions.contains_key(&command.argv[program].text)
                && crate::targets::infer(command, &command.cwd, self.frontend.host)
                    .gaps
                    .iter()
                    .any(|g| matches!(g, CoverageGap::UnknownProgram { .. }))
            {
                if *defining {
                    command.function = true;
                } else {
                    self.output.gap(CoverageGap::UnsupportedShellSyntax);
                }
            }
        }
    }
    pub fn run(
        &mut self,
        body: &[Statement],
        scope: &mut Scope,
        depth: usize,
        source_id: usize,
        nested: bool,
    ) -> Result<(), CheckError> {
        if depth > MAX_NESTING {
            self.output.gap(CoverageGap::InspectionBudget);
            return Ok(());
        }
        for statement in body {
            self.inspected += 1;
            if self.inspected > 512 {
                self.output.gap(CoverageGap::InspectionBudget);
                return Ok(());
            }
            self.statement(statement, scope, depth, source_id, nested)?;
        }
        Ok(())
    }
    fn merge_bindings(&mut self, scope: &mut Scope, branches: &[Scope]) {
        if scope.join(branches) {
            self.output.gap(CoverageGap::InspectionBudget);
        }
    }
    fn merge_directories(&mut self, scope: &mut Scope, branches: &[Scope]) {
        let mut candidates = Vec::new();
        for branch in branches {
            candidates.push(branch.directory.current.clone());
            candidates.extend(branch.directory.alternatives.clone());
        }
        scope
            .directory
            .merge(candidates.into_iter(), self.frontend.host.home);
    }
    fn statement(
        &mut self,
        statement: &Statement,
        scope: &mut Scope,
        depth: usize,
        source_id: usize,
        nested: bool,
    ) -> Result<(), CheckError> {
        // Keep compound temporaries off the recursively entered command frame.
        let Statement::Command {
            assignments,
            argv,
            redirects,
            pipeline,
        } = statement
        else {
            return self.compound_statement(statement, scope, depth, source_id, nested);
        };
        let mut prefixes = Vec::new();
        let mut assignment_scope = scope.clone();
        let mut command_bindings = BTreeMap::new();
        for (name, raw) in assignments {
            let values = self.expand(raw, &mut assignment_scope, depth)?;
            for value in &values {
                self.armed_references(&value.word.text, &mut assignment_scope, depth)?;
            }
            if !argv.is_empty()
                && values
                    .iter()
                    .any(|v| v.word.vars.iter().any(|n| command_bindings.contains_key(n)))
            {
                self.output.gap(CoverageGap::UnsupportedShellSyntax);
            }
            let binding = values
                .iter()
                .map(|v| {
                    if v.word.expands {
                        None
                    } else {
                        Some(v.word.text.clone())
                    }
                })
                .collect::<Vec<_>>();
            assignment_scope.assign(name.clone(), binding.clone());
            if argv.is_empty() {
                scope.assign(name.clone(), binding.clone());
            } else {
                command_bindings.insert(name.clone(), binding);
            }
            let mut word = values
                .first()
                .map(|v| v.word.clone())
                .unwrap_or_else(|| crate::record::Word::literal(String::new()));
            word.text = format!("{name}={}", word.text);
            for range in &mut word.cwd_ranges {
                range.start += name.len() + 1;
                range.end += name.len() + 1;
            }
            word.value = word.text.clone();
            word.role = if argv.is_empty() {
                Role::Precommand
            } else {
                Role::Assign
            };
            prefixes.push(word);
        }
        let mut alternatives = vec![prefixes];
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
                    for range in &mut word.cwd_ranges {
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
                    choices.push(vec![expanded.word]);
                }
            }
            choices.dedup();
            let mut next = Vec::new();
            for previous in &alternatives {
                for choice in &choices {
                    if next.len() == 512 {
                        self.output.gap(CoverageGap::InspectionBudget);
                        break;
                    }
                    let mut argv = previous.clone();
                    argv.extend(choice.clone());
                    next.push(argv);
                }
            }
            alternatives = next;
        }
        let mut targets = Vec::new();
        for redirect in redirects {
            for expanded in self.expand(&redirect.target, scope, depth)? {
                targets.push(crate::record::Redirect::from_word(
                    expanded.word,
                    redirect.direction,
                ));
            }
        }
        let entry = scope.clone();
        let mut exits = Vec::new();
        for argv in alternatives {
            let mut branch = entry.clone();
            let scope = &mut branch;
            let program = (!argv[assignments.len()..].is_empty()).then_some(assignments.len());
            let prior = command_bindings
                .keys()
                .map(|name| (name.clone(), scope.bindings.get(name).cloned()))
                .collect::<BTreeMap<_, _>>();
            let return_start = scope.returns.len();
            for (name, values) in &command_bindings {
                scope.assign(name.clone(), values.clone());
            }
            if let Some(index) = program {
                for word in &argv[index + 1..] {
                    self.armed_word(word, scope, depth)?;
                }
            }
            if let Some(index) = program.filter(|i| !self.functions.contains_key(&argv[*i].text)) {
                if argv[index].expands && !self.functions.is_empty() {
                    self.output.gap(CoverageGap::UnsupportedShellSyntax);
                }
                if argv[index] == "let" {
                    for word in &argv[index + 1..] {
                        self.arithmetic(&word.text, scope, depth)?;
                    }
                }
                self.declaration(&argv[index..], scope);
                match argv[index].text.as_str() {
                    "break" | "continue" => {
                        let state = scope.state();
                        let level = if argv.len() == index + 1 {
                            Some(1)
                        } else if argv.len() == index + 2 {
                            argv[index + 1].text.parse::<usize>().ok()
                        } else {
                            None
                        };
                        if level == Some(1)
                            && prior.is_empty()
                            && let Some(exits) = scope.loops.last_mut()
                        {
                            exits.push(state);
                        } else {
                            self.output.gap(CoverageGap::UnsupportedShellSyntax);
                        }
                    }
                    "return" => {
                        if scope.frames.is_empty() {
                            self.output.gap(CoverageGap::UnsupportedShellSyntax);
                        } else {
                            scope.returns.push(scope.state());
                        }
                    }
                    "eval" => {
                        if argv[index + 1..].iter().any(|word| word.expands) {
                            self.output.gap(CoverageGap::UnsupportedShellSyntax);
                        } else {
                            let before = scope.state();
                            self.source(
                                &argv[index + 1..]
                                    .iter()
                                    .map(|word| word.text.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" "),
                                scope,
                                depth + 1,
                            )?;
                            // Binding-changing eval and function/prefix interactions need a fuller model.
                            if !scope.frames.is_empty()
                                || !prior.is_empty()
                                || scope.state() != before
                            {
                                self.output.gap(CoverageGap::UnsupportedShellSyntax);
                            }
                        }
                    }
                    "read" => {
                        for word in &argv[index + 1..] {
                            if identifier(&word.text) {
                                scope.assign(word.text.clone(), vec![None]);
                            }
                        }
                        self.output.gap(CoverageGap::UnsupportedShellSyntax);
                    }
                    _ => {}
                }
            }
            let command = Command {
                function: program.is_some_and(|i| self.functions.contains_key(&argv[i].text)),
                argv,
                redirects: targets.clone(),
                cwd: scope.directory.current.render(),
                program,
                wrappers: Vec::new(),
                shell: true,
                flags: Vec::new(),
                items: None,
                stdin: Stdin::None,
                pipeline: pipeline.map(|id| (source_id, id)),
                nested,
            };
            self.emit(command.clone(), scope);
            if let Some(name) = program
                .and_then(|i| command.argv.get(i))
                .map(|w| w.text.clone())
                && let Some(function) = self.functions.get(&name).cloned()
            {
                if matches!(name.as_str(), "local" | "declare" | "typeset" | "export") {
                    self.output.gap(CoverageGap::UnsupportedShellSyntax);
                }
                if self.running.contains(&name) || self.function_runs == 256 {
                    self.output.gap(CoverageGap::InspectionBudget);
                } else {
                    self.function_runs += 1;
                    self.running.insert(name.clone());
                    let caller_returns = std::mem::take(&mut scope.returns);
                    let caller_loops = std::mem::take(&mut scope.loops);
                    let caller_failures = scope.directory.failures.take();
                    scope.enter_function();
                    self.run(&function.body, scope, depth + 1, function.source_id, nested)?;
                    let returns = std::mem::take(&mut scope.returns);
                    let mut candidates = vec![scope.clone()];
                    candidates.extend(returns.into_iter().map(|state| scope.with_state(state)));
                    for candidate in &mut candidates {
                        candidate.leave_function();
                    }
                    scope.leave_function();
                    self.merge_bindings(scope, &candidates);
                    scope.returns = caller_returns;
                    scope.loops = caller_loops;
                    scope.directory.failures = caller_failures;
                    self.running.remove(&name);
                }
            }
            if !command.function {
                self.track(&command, scope);
            }
            if !prior.is_empty() {
                restore_prefix(&mut scope.bindings, &prior);
                for state in scope.returns.iter_mut().skip(return_start) {
                    restore_prefix(&mut state.bindings, &prior);
                }
            }
            exits.push(branch);
        }
        if let Some(first) = exits.first() {
            scope.directory = first.directory.clone();
            if let Some(failures) = &mut scope.directory.failures {
                for branch in exits.iter().skip(1) {
                    failures.extend(branch.directory.failures.clone().unwrap_or_default());
                }
            }
        }
        self.merge_directories(scope, &exits);
        self.merge_bindings(scope, &exits);
        Ok(())
    }
    fn compound_statement(
        &mut self,
        statement: &Statement,
        scope: &mut Scope,
        depth: usize,
        source_id: usize,
        nested: bool,
    ) -> Result<(), CheckError> {
        match statement {
            Statement::UnsupportedSyntax => self.output.gap(CoverageGap::UnsupportedShellSyntax),
            Statement::Group(body) => self.run(body, scope, depth + 1, source_id, nested)?,
            Statement::Subshell(body) | Statement::Async(body) => {
                self.run(body, &mut scope.isolated(), depth + 1, source_id, nested)?
            }
            Statement::Substitution(body) => {
                self.run(body, &mut scope.isolated(), depth + 1, source_id, true)?
            }
            Statement::Definition(name, body) => {
                if scope.isolated || scope.conditional_definition || !scope.frames.is_empty() {
                    self.output.gap(CoverageGap::UnsupportedShellSyntax);
                }
                if self
                    .functions
                    .insert(
                        name.clone(),
                        Function {
                            body: body.clone(),
                            source_id,
                        },
                    )
                    .is_some()
                {
                    self.output.gap(CoverageGap::UnsupportedShellSyntax);
                }
                let mut inner = scope.isolated();
                inner.defining = true;
                inner.enter_function();
                let inserted = self.running.insert(name.clone());
                self.run(body, &mut inner, depth + 1, source_id, nested)?;
                if inserted {
                    self.running.remove(name);
                }
            }
            Statement::Binary(operator, left, right) => {
                let before = scope.isolated();
                match operator {
                    Operator::And => {
                        let qualified = cwd::moved_on_success(left);
                        let outer_failures = if qualified {
                            scope.directory.failures.take()
                        } else {
                            None
                        };
                        if qualified {
                            scope.directory.failures = Some(Vec::new());
                        }
                        self.statement(left, scope, depth + 1, source_id, nested)?;
                        let left_exit = scope.clone();
                        let mut after = scope.branch();
                        if qualified && !matches!(right.as_ref(), Statement::Command { .. }) {
                            after.directory.failures = None;
                        }
                        self.statement(right, &mut after, depth + 1, source_id, nested)?;
                        let mut failures = left_exit.directory.failures.clone().unwrap_or_default();
                        failures.extend(after.directory.failures.clone().unwrap_or_default());
                        scope.directory = after.directory.clone();
                        if qualified {
                            scope.directory.failures = outer_failures.map(|mut outer| {
                                outer.extend(failures.clone());
                                outer
                            });
                            if scope.directory.failures.is_none() {
                                scope
                                    .directory
                                    .merge(failures.into_iter(), self.frontend.host.home);
                            }
                        }
                        self.merge_bindings(scope, &[before, left_exit, after]);
                    }
                    Operator::Or | Operator::Pipe => {
                        if matches!(operator, Operator::Or) {
                            self.statement(left, scope, depth + 1, source_id, nested)?;
                        } else {
                            self.statement(
                                left,
                                &mut before.isolated(),
                                depth + 1,
                                source_id,
                                nested,
                            )?;
                        }
                        let mut rhs = before.isolated();
                        self.statement(right, &mut rhs, depth + 1, source_id, nested)?;
                        let lhs = scope.clone();
                        self.merge_bindings(scope, &[lhs, rhs.clone()]);
                        self.merge_directories(scope, &[rhs]);
                    }
                }
            }
            Statement::Conditional {
                condition,
                then,
                otherwise,
            } => {
                let before = scope.clone();
                let mut test = scope.clone();
                test.directory.failures = None;
                self.run(condition, &mut test, depth + 1, source_id, nested)?;
                let mut yes = test.branch();
                self.run(then, &mut yes, depth + 1, source_id, nested)?;
                let mut no = test.branch();
                self.run(otherwise, &mut no, depth + 1, source_id, nested)?;
                self.merge_directories(
                    scope,
                    &[before.clone(), test.clone(), yes.clone(), no.clone()],
                );
                let branches = if otherwise.is_empty() {
                    vec![before, test, yes, no]
                } else {
                    vec![yes, no]
                };
                self.merge_bindings(scope, &branches);
            }
            Statement::Case {
                words,
                branches,
                exhaustive,
            } => {
                for word in words {
                    self.word_use(word, scope, depth, nested)?;
                }
                let mut exits = Vec::new();
                if !exhaustive {
                    exits.push(scope.clone());
                }
                for body in branches {
                    let mut inner = scope.branch();
                    self.run(body, &mut inner, depth + 1, source_id, nested)?;
                    exits.push(inner);
                }
                self.merge_directories(scope, &exits);
                self.merge_bindings(scope, &exits);
            }
            Statement::Loop {
                variable,
                header,
                body,
                empty,
            } => {
                let before = scope.clone();
                let mut inner = scope.branch();
                inner.directory.failures = None;
                let mut values = Vec::new();
                let mut count = Some(0usize);
                for word in header {
                    let mut width = 0;
                    for expanded in self.expand(word, scope, depth)? {
                        width = width.max(expanded.split.len().max(1));
                        if expanded.word.expands || expanded.word.globs {
                            count = None;
                        }
                        values.extend(expanded.split.into_iter().map(|w| {
                            if w.expands || w.globs {
                                None
                            } else {
                                Some(w.text)
                            }
                        }));
                        let unsplit = if expanded.word.expands || expanded.word.globs {
                            None
                        } else {
                            Some(expanded.word.text)
                        };
                        if !values.contains(&unsplit) {
                            values.push(unsplit);
                        }
                    }
                    count = count.map(|n| n + width);
                    self.word_use(word, scope, depth, nested)?;
                }
                let iterations = if *empty {
                    Some(0)
                } else if variable.is_some() && !header.is_empty() {
                    count
                } else {
                    None
                };
                if let Some(variable) = variable {
                    inner.assign(
                        variable.clone(),
                        if values.is_empty() {
                            vec![None]
                        } else {
                            values
                        },
                    );
                }
                inner.loops.push(Vec::new());
                self.run(body, &mut inner, depth + 1, source_id, nested)?;
                let first = inner.clone();
                if !matches!(iterations, Some(0 | 1)) {
                    self.run(body, &mut inner, depth + 1, source_id, nested)?;
                    if inner.bindings != first.bindings && iterations.is_none_or(|n| n > 2) {
                        self.output.gap(CoverageGap::InspectionBudget);
                    }
                }
                let early = inner.loops.pop().ok_or(CheckError {
                    kind: crate::CheckErrorKind::GuardFault,
                })?;
                let mut branches = vec![before, first, inner.clone()];
                branches.extend(early.into_iter().map(|state| inner.with_state(state)));
                if *empty {
                    branches.truncate(1);
                }
                self.merge_directories(scope, &branches);
                self.merge_bindings(scope, &branches);
            }
            Statement::Use(word) => self.word_use(word, scope, depth, nested)?,
            Statement::Expansion(word) => {
                self.expand(word, scope, depth)?;
            }
            Statement::Command { .. } => {
                self.statement(statement, scope, depth, source_id, nested)?;
            }
        }
        Ok(())
    }
    fn declaration(&mut self, argv: &[crate::record::Word], scope: &mut Scope) {
        let Some(program) = argv.first().map(|w| w.text.as_str()) else {
            return;
        };
        if !matches!(program, "export" | "local" | "declare" | "typeset") {
            return;
        }
        let in_function = !scope.frames.is_empty();
        let local = in_function && matches!(program, "local" | "declare" | "typeset");
        let top_local = !in_function && program == "local";
        if in_function && (program == "export" || argv.iter().skip(1).any(|w| w.starts_with('-'))) {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
        }
        for word in &argv[1..] {
            if let Some((name, value)) = assignment(&word.text) {
                if local {
                    scope.local(name);
                }
                let mut values = vec![if word.expands {
                    None
                } else {
                    Some(value.into())
                }];
                if top_local {
                    let prior = scope
                        .bindings
                        .get(name)
                        .map_or_else(|| vec![None], |b| b.values.clone());
                    for value in prior {
                        if !values.contains(&value) {
                            values.push(value);
                        }
                    }
                    if values.contains(&None) {
                        self.output.gap(CoverageGap::UnsupportedShellSyntax);
                    }
                }
                scope.assign(name.into(), values);
            } else if local && identifier(&word.text) {
                scope.local(&word.text);
                scope.assign(word.text.clone(), vec![None]);
            }
        }
    }
    fn word_use(
        &mut self,
        raw: &RawWord,
        scope: &mut Scope,
        depth: usize,
        nested: bool,
    ) -> Result<(), CheckError> {
        for expanded in self.expand(raw, scope, depth)? {
            self.emit(
                Command {
                    function: false,
                    argv: vec![expanded.word],
                    redirects: Vec::new(),
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
            );
        }
        Ok(())
    }
    fn emit(&mut self, mut command: Command, scope: &Scope) {
        let data = command
            .redirects
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                matches!(
                    r.direction,
                    crate::record::Direction::Heredoc | crate::record::Direction::Herestring
                )
                .then_some(i)
            })
            .collect::<Vec<_>>();
        if !data.is_empty() {
            command.stdin = Stdin::Data(data);
        }
        if command.program.is_some() && !command.function {
            self.unresolved_calls
                .push((self.output.script.commands.len(), scope.defining));
        }
        self.output.script.commands.push(command.clone());
        for cwd in &scope.directory.alternatives {
            let cwd = cwd.render();
            let mut copy = command.clone();
            copy.cwd = cwd.clone();
            for word in &mut copy.argv {
                if word.pwd {
                    word.reproject_cwd(&cwd);
                }
            }
            if copy.program.is_some() && !copy.function {
                self.unresolved_calls
                    .push((self.output.script.commands.len(), scope.defining));
            }
            self.output.script.commands.push(copy);
        }
        if self.output.script.commands.len() > 512 {
            self.output.gap(CoverageGap::InspectionBudget);
        }
    }
    fn track(&mut self, command: &Command, scope: &mut Scope) {
        let Some(index) = command.program else {
            return;
        };
        let program = command.argv[index].text.as_str();
        if !command.shell || !matches!(program, "cd" | "pushd") {
            return;
        }
        let args = &command.argv[index + 1..];
        let operand = args.iter().position(|w| !w.starts_with('-'));
        let target = if let Some(operand) = operand {
            args[operand].text.clone()
        } else if program == "cd"
            && args.iter().all(|w| {
                w == "--"
                    || w.strip_prefix('-').is_some_and(|flags| {
                        !flags.is_empty() && flags.bytes().all(|b| b"PLqs".contains(&b))
                    })
            })
        {
            self.frontend.host.home.into()
        } else {
            return;
        };
        let modes = args[..operand.unwrap_or(args.len())]
            .iter()
            .filter(|w| {
                w.strip_prefix('-').is_some_and(|flags| {
                    !flags.is_empty() && flags.bytes().all(|b| b.is_ascii_alphabetic())
                })
            })
            .flat_map(|w| w.bytes().filter(|b| matches!(b, b'L' | b'P')))
            .collect::<Vec<_>>();
        let physical =
            modes.contains(&b'P') && operand.is_none_or(|i| !args[i].expands && !args[i].globs);
        let disputed = physical && modes.last() == Some(&b'L');
        let mut targets = vec![target.clone()];
        if operand.is_some()
            && !target.starts_with(['/', '.'])
            && let Some(binding) = scope.bindings.get("CDPATH")
        {
            for value in &binding.values {
                if let Some(value) = value {
                    for entry in value.split(':').filter(|entry| !entry.is_empty()) {
                        let path = format!("{entry}/{target}");
                        if !targets.contains(&path) {
                            targets.push(path);
                        }
                    }
                } else {
                    self.output.gap(CoverageGap::UnsupportedShellSyntax);
                }
            }
        }
        scope
            .directory
            .move_to(&targets, physical, disputed, self.frontend.host.home);
        scope.bindings.remove("PWD");
    }
    pub fn expand(
        &mut self,
        raw: &RawWord,
        scope: &mut Scope,
        depth: usize,
    ) -> Result<Vec<Expanded>, CheckError> {
        super::expand_scoped(raw, scope, self, depth)
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
            self.source(&code, &mut scope.isolated(), depth + 1)?;
        }
        Ok(())
    }
    pub fn arithmetic_code(
        &mut self,
        expression: &str,
        scope: &Scope,
    ) -> Result<Vec<String>, CheckError> {
        let evaluation = super::arithmetic::evaluate(expression, &scope.values())?;
        if evaluation.bounded {
            self.output.gap(CoverageGap::InspectionBudget);
        }
        if evaluation.names.iter().any(|name| {
            scope
                .bindings
                .get(name)
                .is_some_and(|b| b.values.contains(&None))
        }) {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
        }
        Ok(evaluation.code)
    }
    fn armed_word(
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
        let names = match super::arithmetic::evaluate(expression, &BTreeMap::new()) {
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
    fn armed_reference(
        &mut self,
        name: &str,
        scope: &mut Scope,
        depth: usize,
    ) -> Result<(), CheckError> {
        let Some(binding) = scope.bindings.get(name) else {
            return Ok(());
        };
        let mut sources = Vec::new();
        for value in &binding.values {
            let Some(value) = value else {
                self.output.gap(CoverageGap::UnsupportedShellSyntax);
                continue;
            };
            match super::arithmetic::armed(value) {
                super::arithmetic::Arming::Armed(code) => {
                    for source in code {
                        if !sources.contains(&source) {
                            sources.push(source);
                        }
                    }
                }
                super::arithmetic::Arming::Inert => {}
                super::arithmetic::Arming::Unresolved => {
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
                self.source(&source, &mut scope.isolated(), depth + 1)?;
            }
        }
        Ok(())
    }
}

pub(super) fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || i > 0 && b.is_ascii_digit())
}
fn assignment(raw: &str) -> Option<(&str, &str)> {
    raw.split_once('=').filter(|(n, _)| identifier(n))
}

fn indexed_name(name: &str) -> Option<&str> {
    let (variable, tail) = name.split_once('[')?;
    identifier(variable).then_some(())?;
    tail.strip_suffix(']')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CoverageGap, record::HostFacts, shell::Arm};
    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/rust-m2-scope-bounds.json"
        ))
        .unwrap()
    }
    fn seeded_scope() -> Scope {
        let values = fixture()["values"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| Some(v.as_str().unwrap().into()))
            .collect::<Vec<_>>();
        let mut scope = Scope::new("/h", "/p");
        for name in ["A", "B", "C"] {
            scope.assign(name.into(), values.clone());
        }
        scope
    }
    fn observation(source: &str, scope: &mut Scope) -> Observation {
        let mut output = Observation::default();
        let frontend = Frontend {
            arm: Arm::Brush,
            zsh: true,
            host: HostFacts {
                home: "/h",
                user: None,
            },
        };
        let mut evaluator = Evaluator::new(frontend, &mut output);
        evaluator.source(source, scope, 0).unwrap();
        evaluator.finish();
        output
    }
    #[test]
    fn binding_join_replaces_the_complete_exit_state() {
        let mut outer = Scope::new("/h", "/p");
        let exit = outer.clone();
        outer.assign("D".into(), vec![Some("public".into())]);
        assert!(!outer.join(&[exit]));
        assert!(!outer.contexts().contains_key("D"));
    }
    #[test]
    fn function_replay_has_its_own_work_bound() {
        let data = fixture();
        let result = observation(
            data["sources"]["function_runs"].as_str().unwrap(),
            &mut seeded_scope(),
        );
        assert!(result.script.commands.len() < 512);
        assert!(result.gaps.contains(&CoverageGap::InspectionBudget));
    }
    #[test]
    fn recursive_function_does_not_expand_to_the_depth_bound() {
        let data = fixture();
        let result = observation(
            data["sources"]["recursive"].as_str().unwrap(),
            &mut Scope::new("/h", "/p"),
        );
        assert!(result.script.commands.len() <= 3);
        assert!(result.gaps.contains(&CoverageGap::InspectionBudget));
    }
    #[test]
    fn word_context_product_has_a_finite_bound() {
        let data = fixture();
        let raw = RawWord {
            raw: data["sources"]["contexts"].as_str().unwrap().into(),
            syntax: WordSyntax::Shell,
            expansions: Vec::new(),
        };
        let mut scope = seeded_scope();
        let mut output = Observation::default();
        let frontend = Frontend {
            arm: Arm::Brush,
            zsh: true,
            host: HostFacts {
                home: "/h",
                user: None,
            },
        };
        let expanded = Evaluator::new(frontend, &mut output)
            .expand(&raw, &mut scope, 0)
            .unwrap();
        assert_eq!(expanded.len(), 512);
        assert!(output.gaps.contains(&CoverageGap::InspectionBudget));
    }
    #[test]
    fn command_argv_product_has_a_finite_bound() {
        let data = fixture();
        let result = observation(
            data["sources"]["argv"].as_str().unwrap(),
            &mut seeded_scope(),
        );
        assert_eq!(result.script.commands.len(), 512);
        assert!(result.gaps.contains(&CoverageGap::InspectionBudget));
    }
    #[test]
    fn statement_work_bound_applies_without_emitted_commands() {
        let data = fixture();
        let source = data["sources"]["statement"].as_str().unwrap().repeat(600);
        let result = observation(&source, &mut Scope::new("/h", "/p"));
        assert!(result.script.commands.is_empty());
        assert!(result.gaps.contains(&CoverageGap::InspectionBudget));
    }
    #[test]
    fn unknown_binding_at_an_arithmetic_sink_refuses() {
        let data = fixture();
        let mut scope = Scope::new("/h", "/p");
        scope.assign("A".into(), vec![None]);
        let result = observation(
            data["sources"]["unknown_arithmetic"].as_str().unwrap(),
            &mut scope,
        );
        assert!(result.gaps.contains(&CoverageGap::UnsupportedShellSyntax));
    }
    #[test]
    fn binding_values_and_exit_snapshots_are_bounded() {
        let mut outer = Scope::new("/h", "/p");
        outer.loops.push(Vec::new());
        let branches = (0..513)
            .map(|n| {
                let mut branch = outer.clone();
                branch.assign("A".into(), vec![Some(n.to_string())]);
                let state = branch.state();
                branch.returns.push(state.clone());
                branch.loops[0].push(state);
                branch
            })
            .collect::<Vec<_>>();
        assert!(outer.join(&branches));
        assert_eq!(outer.bindings["A"].values.len(), 512);
        assert_eq!(outer.returns.len(), 512);
        assert_eq!(outer.loops[0].len(), 512);
    }
    #[test]
    fn directory_alternatives_keep_order_and_collapse() {
        let paths = (0..17)
            .map(|n| cwd::CwdPath::Logical(format!("/p/{n}")))
            .collect::<Vec<_>>();
        let current = cwd::CwdPath::Logical("/p".into());
        assert_eq!(
            cwd::bounded(&current, paths[..16].to_vec(), "/h"),
            paths[..16]
        );
        assert_eq!(
            cwd::bounded(&current, paths, "/h"),
            [
                cwd::CwdPath::Logical("/h".into()),
                cwd::CwdPath::Logical("/h/Library".into())
            ]
        );
    }
}
