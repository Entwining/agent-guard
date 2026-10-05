use super::cwd::{self, Directory};
use super::{Expanded, Frontend, Observation, Operator, RawWord, Statement, WordSyntax};
use crate::{
    CheckError, CoverageGap,
    limits::MAX_NESTING,
    record::{Command, Role, Stdin},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum BindingValue {
    Known(String),
    // The lexical representative preserves pre-M2 target inference; it is
    // never evidence of the runtime value or its arithmetic contents.
    RuntimeUnknown(Option<String>),
    // Derived text must carry its runtime uncertainty through substitution.
    RuntimeDerived(String),
    Undetermined,
}

impl BindingValue {
    fn lexical(&self) -> Option<&String> {
        match self {
            Self::Known(value)
            | Self::RuntimeUnknown(Some(value))
            | Self::RuntimeDerived(value) => Some(value),
            Self::RuntimeUnknown(None) | Self::Undetermined => None,
        }
    }
    pub fn known(&self) -> Option<&String> {
        match self {
            Self::Known(value) => Some(value),
            Self::RuntimeUnknown(_) | Self::RuntimeDerived(_) | Self::Undetermined => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Binding {
    pub values: Vec<BindingValue>,
    exported: bool,
    arithmetic: bool,
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
                    values: vec![BindingValue::Known(home.into())],
                    exported: false,
                    arithmetic: false,
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
    fn candidates(&self) -> BTreeMap<String, Vec<BindingValue>> {
        let mut values: BTreeMap<String, Vec<BindingValue>> = BTreeMap::new();
        for (name, binding) in &self.bindings {
            let base = name.split_once('[').map_or(name.as_str(), |(base, _)| base);
            values
                .entry(base.into())
                .or_default()
                .extend(binding.values.clone());
        }
        values
    }
    pub fn values(&self) -> BTreeMap<String, Vec<String>> {
        self.candidates()
            .into_iter()
            .map(|(name, values)| {
                (
                    name,
                    values
                        .iter()
                        .filter_map(BindingValue::known)
                        .cloned()
                        .collect(),
                )
            })
            .collect()
    }
    pub fn contexts(&self) -> BTreeMap<String, String> {
        self.bindings
            .iter()
            .filter_map(|(n, b)| {
                b.values
                    .first()
                    .and_then(BindingValue::lexical)
                    .map(|v| (n.clone(), v.clone()))
            })
            .collect::<BTreeMap<_, _>>()
    }
    fn expanded_binding(&self, word: &crate::record::Word, text: &str) -> BindingValue {
        let candidates = word.vars.iter().filter_map(|name| self.bindings.get(name));
        let mut runtime = false;
        let mut representative = false;
        let mut derived = false;
        for binding in candidates {
            for value in &binding.values {
                match value {
                    BindingValue::RuntimeDerived(_) => derived = true,
                    BindingValue::RuntimeUnknown(Some(value)) => {
                        runtime |= !value.is_empty()
                            || !binding.values.iter().any(|value| value.known().is_some());
                        representative |= value == text;
                    }
                    BindingValue::RuntimeUnknown(None) => {
                        runtime = true;
                    }
                    _ => {}
                }
            }
        }
        if derived || (runtime && (word.globs || !representative)) {
            BindingValue::RuntimeDerived(text.into())
        } else if runtime || word.expands {
            BindingValue::RuntimeUnknown(Some(text.into()))
        } else {
            BindingValue::Known(text.into())
        }
    }
    fn local(&mut self, name: &str) {
        if let Some(frame) = self.frames.last_mut() {
            frame
                .entry(name.into())
                .or_insert_with(|| self.bindings.get(name).cloned());
            let prefix = format!("{name}[");
            let indexed = self
                .bindings
                .keys()
                .filter(|key| key.starts_with(&prefix))
                .cloned()
                .collect::<Vec<_>>();
            for key in indexed {
                frame
                    .entry(key.clone())
                    .or_insert_with(|| self.bindings.get(&key).cloned());
                self.bindings.remove(&key);
            }
        }
    }
    fn assign(&mut self, name: String, values: Vec<BindingValue>) {
        if let Some((base, _)) = name.split_once('[')
            && let Some(frame) = self
                .frames
                .iter_mut()
                .rev()
                .find(|frame| frame.contains_key(base))
        {
            frame
                .entry(name.clone())
                .or_insert_with(|| self.bindings.get(&name).cloned());
        }
        let exported = self
            .bindings
            .get(&name)
            .is_some_and(|binding| binding.exported);
        let arithmetic = self
            .bindings
            .get(&name)
            .is_some_and(|binding| binding.arithmetic);
        self.bindings.insert(
            name,
            Binding {
                values,
                exported,
                arithmetic,
            },
        );
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
    let mut exported = false;
    let mut arithmetic = false;
    for binding in values {
        present |= binding.is_some();
        exported |= binding.is_some_and(|binding| binding.exported);
        arithmetic |= binding.is_some_and(|binding| binding.arithmetic);
        let values = binding.map_or_else(
            || vec![BindingValue::RuntimeUnknown(Some(String::new()))],
            |b| b.values.clone(),
        );
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
    (
        present.then_some(Binding {
            values: joined,
            exported,
            arithmetic,
        }),
        bounded,
    )
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
            scope.directory.gap = scope.directory.gap.take().or(branch.directory.gap.clone());
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
            let observe_bindings = assignment_scope
                .bindings
                .get(name.strip_suffix('+').unwrap_or(name))
                .is_some_and(|binding| binding.arithmetic);
            // Copying stores the armed value; only an arithmetic attribute
            // consumes it here. A later assignment replaces the stored value.
            let values =
                super::expand_scoped(raw, &mut assignment_scope, self, depth, observe_bindings)?;
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
            let mut binding = values
                .iter()
                .map(|v| assignment_scope.expanded_binding(&v.word, &v.word.text))
                .collect::<Vec<_>>();
            let (name, append) = name
                .strip_suffix('+')
                .map_or((name.as_str(), false), |name| (name, true));
            if append {
                let prior = assignment_scope.bindings.get(name).map_or_else(
                    || vec![BindingValue::Known(String::new())],
                    |binding| binding.values.clone(),
                );
                let mut combined = Vec::new();
                for left in &prior {
                    for right in &binding {
                        if combined.len() == 512 {
                            self.output.gap(CoverageGap::InspectionBudget);
                            break;
                        }
                        let value = match (left, right) {
                            (BindingValue::Known(left), BindingValue::Known(right)) => {
                                BindingValue::Known(format!("{left}{right}"))
                            }
                            (BindingValue::Undetermined, _) | (_, BindingValue::Undetermined) => {
                                BindingValue::Undetermined
                            }
                            _ => BindingValue::RuntimeDerived(format!(
                                "{}{}",
                                left.lexical().map_or("", String::as_str),
                                right.lexical().map_or("", String::as_str),
                            )),
                        };
                        if !combined.contains(&value) {
                            combined.push(value);
                        }
                    }
                }
                binding = combined;
            }
            assignment_scope.assign(name.to_owned(), binding.clone());
            if argv.is_empty() {
                scope.assign(name.to_owned(), binding.clone());
            } else {
                command_bindings.insert(name.to_owned(), binding);
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
        for mut argv in alternatives {
            let mut branch = entry.clone();
            let scope = &mut branch;
            let resolved = super::argv::resolve(
                &mut argv,
                &scope.directory.current.render(),
                self.frontend.host,
            );
            let program = resolved.program;
            if let Some(gap) = resolved.gap {
                self.output.gap(gap);
            }
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
                    // These builtins consume variable names, not arithmetic values.
                    if !(resolved.shell
                        && !self.functions.contains_key(&argv[index].text)
                        && matches!(argv[index].text.as_str(), "unset" | "export")
                        && identifier(&word.text))
                    {
                        self.armed_word(word, scope, depth)?;
                    }
                }
            }
            if let Some(index) = program.filter(|i| {
                !self.functions.contains_key(&argv[*i].text)
                    && (resolved.shell
                        || argv[*i] == "eval" && resolved.wrappers.iter().any(|w| w == "command"))
            }) {
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
                    "unset" => self.unset(&argv[index + 1..], scope),
                    "set" if argv.get(index + 1).is_some_and(|word| word == "--") => {
                        self.positionals(&argv[index + 2..], scope);
                    }
                    "printf" if argv.get(index + 1).is_some_and(|word| word == "-v") => {
                        if let Some(name) =
                            argv.get(index + 2).filter(|word| identifier(&word.text))
                        {
                            let values = &argv[index + 3..];
                            if values.first().is_some_and(|word| word == "%s")
                                && values.len() == 2
                                && !values[1].expands
                            {
                                scope.assign(
                                    name.text.clone(),
                                    vec![BindingValue::Known(values[1].text.clone())],
                                );
                            } else {
                                let joined = values
                                    .iter()
                                    .skip(1)
                                    .map(|word| word.text.as_str())
                                    .collect::<String>();
                                if !matches!(
                                    super::arithmetic::armed(&joined),
                                    super::arithmetic::Arming::Inert
                                ) {
                                    self.output.gap(CoverageGap::UnsupportedShellSyntax);
                                    scope.assign(
                                        name.text.clone(),
                                        vec![BindingValue::Undetermined],
                                    );
                                } else {
                                    scope.assign(
                                        name.text.clone(),
                                        vec![BindingValue::RuntimeUnknown(None)],
                                    );
                                }
                            }
                        }
                    }
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
                        let names: Vec<_> = argv[index + 1..]
                            .iter()
                            .filter(|word| identifier(&word.text))
                            .collect();
                        let literal = targets.iter().rev().find(|target| {
                            matches!(
                                target.direction,
                                crate::record::Direction::Heredoc
                                    | crate::record::Direction::Herestring
                            ) && !target.expands
                        });
                        let modeled = names.len() == 1
                            && argv[index + 1..]
                                .iter()
                                .all(|word| identifier(&word.text) || word == "-r")
                            && (!scope.bindings.contains_key("IFS"))
                            && (argv.iter().any(|word| word == "-r")
                                || literal.is_none_or(|literal| !literal.target.contains('\\')));
                        for word in &argv[index + 1..] {
                            if identifier(&word.text) {
                                scope.assign(
                                    word.text.clone(),
                                    vec![if modeled && let Some(literal) = literal {
                                        BindingValue::Known(
                                            literal
                                                .target
                                                .lines()
                                                .next()
                                                .unwrap_or("")
                                                .trim_matches([' ', '\t'])
                                                .to_owned(),
                                        )
                                    } else {
                                        BindingValue::RuntimeUnknown(None)
                                    }],
                                );
                            }
                        }
                        if !modeled
                            && literal.is_some_and(|literal| {
                                !matches!(
                                    super::arithmetic::armed(&literal.target.replace('\\', "")),
                                    super::arithmetic::Arming::Inert
                                )
                            })
                        {
                            self.output.gap(CoverageGap::UnsupportedShellSyntax);
                        }
                    }
                    _ => {}
                }
            }
            let command = Command {
                environment: Vec::new(),
                function: resolved.wrappers.is_empty()
                    && program.is_some_and(|i| self.functions.contains_key(&argv[i].text)),
                argv,
                redirects: targets.clone(),
                cwd: resolved.cwd,
                program,
                wrappers: resolved.wrappers,
                shell: resolved.shell,
                flags: Vec::new(),
                items: None,
                stdin: Stdin::None,
                pipeline: pipeline.map(|id| (source_id, id)),
                nested,
            };
            let command = self.emit(command, scope, &resolved.environment);
            if let Some(index) = program.filter(|index| {
                !command.function
                    && matches!(
                        command.argv[*index].rsplit('/').next(),
                        Some("sh" | "bash" | "zsh" | "dash" | "ksh")
                    )
            }) && let Some(pair) = command.argv[index + 1..]
                .windows(2)
                .find(|pair| super::argv::shell_code_flag(&pair[0]))
            {
                let mut child = Scope::new(self.frontend.host.home, &command.cwd);
                child.bindings.extend(
                    scope
                        .bindings
                        .iter()
                        .filter(|(name, binding)| {
                            binding.exported
                                && !matches!(name.as_str(), "GIT_DIR" | "GIT_WORK_TREE")
                        })
                        .map(|(name, binding)| (name.clone(), binding.clone())),
                );
                for (name, value) in &command.environment {
                    let value = if value.expands {
                        BindingValue::RuntimeDerived(value.text.clone())
                    } else {
                        BindingValue::Known(value.text.clone())
                    };
                    let binding = child
                        .bindings
                        .entry(name.clone())
                        .or_insert_with(|| Binding {
                            values: Vec::new(),
                            exported: true,
                            arithmetic: false,
                        });
                    if !binding.values.contains(&value) {
                        binding.values.push(value);
                    }
                }
                let functions = self.functions.clone();
                let result = self.source(&pair[1].text, &mut child, depth + 1);
                self.functions = functions;
                result?;
            }
            if let Some(source) = resolved.source {
                self.isolated_source(&source, scope, depth + 1)?;
            }
            if super::argv::stdin_kind(&command) == Stdin::Shell {
                for redirect in &command.redirects {
                    if matches!(
                        redirect.direction,
                        crate::record::Direction::Heredoc | crate::record::Direction::Herestring
                    ) {
                        self.source(
                            &redirect.target,
                            &mut Scope::new(self.frontend.host.home, &command.cwd),
                            depth + 1,
                        )?;
                    }
                }
            }
            if command.wrappers.iter().any(|w| w == "xargs") && command.program.is_some() {
                for redirect in &command.redirects {
                    if matches!(
                        redirect.direction,
                        crate::record::Direction::Heredoc | crate::record::Direction::Herestring
                    ) {
                        for input in super::pipeline::xargs_here_input(&command, &redirect.target) {
                            self.source(
                                &input.source,
                                &mut Scope::new(self.frontend.host.home, &input.cwd),
                                depth + 1,
                            )?;
                        }
                    }
                }
            }
            if let Some(name) = program
                .and_then(|i| command.argv.get(i))
                .map(|w| w.text.clone())
                && command.function
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
                    self.positionals(&command.argv[program.unwrap_or(0) + 1..], scope);
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
            Statement::ArrayAssignment {
                name,
                values,
                append,
            } => {
                let mut elements = Vec::new();
                for raw in values {
                    let mut candidates = Vec::new();
                    for expanded in self.expand(raw, scope, depth)? {
                        if candidates.len() == 512 {
                            self.output.gap(CoverageGap::InspectionBudget);
                            break;
                        }
                        candidates.push(if expanded.word.expands {
                            BindingValue::RuntimeUnknown(None)
                        } else {
                            BindingValue::Known(expanded.word.text)
                        });
                    }
                    elements.push(candidates);
                }
                let prefix = format!("{name}[");
                let start = if *append {
                    scope
                        .bindings
                        .keys()
                        .filter(|key| key.starts_with(&prefix))
                        .count()
                } else {
                    0
                };
                if !append {
                    scope
                        .bindings
                        .retain(|key, _| key != name && !key.starts_with(&prefix));
                }
                for (index, candidates) in elements.into_iter().enumerate() {
                    scope.assign(format!("{name}[{}]", start + index), candidates);
                }
            }
            Statement::UnsupportedSyntax => self.output.gap(CoverageGap::UnsupportedShellSyntax),
            Statement::Group(body) => self.run(body, scope, depth + 1, source_id, nested)?,
            Statement::Subshell(body) | Statement::Async(body) | Statement::Substitution(body) => {
                let functions = self.functions.clone();
                let result = self.run(
                    body,
                    &mut scope.isolated(),
                    depth + 1,
                    source_id,
                    nested || matches!(statement, Statement::Substitution(_)),
                );
                self.functions = functions;
                result?;
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
                        let start = self.output.script.commands.len();
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
                        let middle = self.output.script.commands.len();
                        let mut rhs = if matches!(operator, Operator::Or) {
                            scope.branch()
                        } else {
                            before.isolated()
                        };
                        self.statement(right, &mut rhs, depth + 1, source_id, nested)?;
                        if matches!(operator, Operator::Pipe) {
                            let (left, right) =
                                self.output.script.commands[start..].split_at_mut(middle - start);
                            super::pipeline::mark_walked_input(left, right);
                            let mut sources = super::pipeline::xargs_replacements(left, right);
                            sources.extend(super::pipeline::shell_input(left, right));
                            for input in sources {
                                self.source(
                                    &input.source,
                                    &mut Scope::new(self.frontend.host.home, &input.cwd),
                                    depth + 1,
                                )?;
                            }
                        }
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
                let mut literal = variable.is_some() && !header.is_empty();
                let mut literal_values = Vec::new();
                let mut count = Some(0usize);
                for word in header {
                    let mut width = 0;
                    for expanded in self.expand(word, scope, depth)? {
                        literal &= word.expansions.is_empty()
                            && expanded.word.vars.is_empty()
                            && !expanded.word.expands
                            && !expanded.word.globs
                            && !expanded.tilde
                            && expanded.nested.is_empty()
                            && expanded.arithmetic.is_empty();
                        literal_values.push(expanded.word.text.clone());
                        width = width.max(expanded.split.len().max(1));
                        if expanded.word.expands || expanded.word.globs {
                            count = None;
                        }
                        values.extend(expanded.split.into_iter().map(|w| {
                            if w.expands {
                                BindingValue::RuntimeUnknown(None)
                            } else if w.globs {
                                BindingValue::RuntimeUnknown(Some(w.text))
                            } else {
                                BindingValue::Known(w.text)
                            }
                        }));
                        let unsplit = if expanded.word.expands {
                            BindingValue::RuntimeUnknown(None)
                        } else if expanded.word.globs {
                            BindingValue::RuntimeUnknown(Some(expanded.word.text))
                        } else {
                            BindingValue::Known(expanded.word.text)
                        };
                        if !values.contains(&unsplit) {
                            values.push(unsplit);
                        }
                    }
                    count = count.map(|n| n + width);
                    self.word_use(word, scope, depth, nested)?;
                }
                if literal
                    && literal_values.len() <= 512
                    && let Some(variable) = variable
                {
                    inner.loops.push(Vec::new());
                    for value in literal_values {
                        inner.assign(variable.clone(), vec![BindingValue::Known(value)]);
                        self.run(body, &mut inner, depth + 1, source_id, nested)?;
                    }
                    let early = inner.loops.pop().ok_or(CheckError {
                        kind: crate::CheckErrorKind::GuardFault,
                    })?;
                    let mut branches = early
                        .into_iter()
                        .map(|state| inner.with_state(state))
                        .collect::<Vec<_>>();
                    branches.push(inner.clone());
                    scope.directory = inner.directory.clone();
                    self.merge_directories(scope, &branches);
                    self.merge_bindings(scope, &branches);
                    return Ok(());
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
                            vec![BindingValue::RuntimeUnknown(None)]
                        } else {
                            values
                        },
                    );
                }
                inner.loops.push(Vec::new());
                self.run(body, &mut inner, depth + 1, source_id, nested)?;
                let mut branches = vec![before, inner.clone()];
                let mut completed = 1;
                while iterations.is_none_or(|n| completed < n) {
                    let prior = inner.clone();
                    self.run(body, &mut inner, depth + 1, source_id, nested)?;
                    completed += 1;
                    branches.push(inner.clone());
                    if inner.bindings == prior.bindings
                        && inner.directory.current == prior.directory.current
                        && inner.directory.alternatives == prior.directory.alternatives
                    {
                        break;
                    }
                    if self.inspected >= 512 || completed >= 512 {
                        self.output.gap(CoverageGap::InspectionBudget);
                        break;
                    }
                }
                let early = inner.loops.pop().ok_or(CheckError {
                    kind: crate::CheckErrorKind::GuardFault,
                })?;
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
    fn unset(&mut self, argv: &[crate::record::Word], scope: &mut Scope) {
        let mut functions = false;
        let mut options = true;
        for word in argv {
            if options && word == "--" {
                options = false;
                continue;
            }
            if options && word.starts_with('-') {
                match word.text.as_str() {
                    "-v" => functions = false,
                    "-f" => functions = true,
                    _ => return,
                }
                continue;
            }
            options = false;
            if !identifier(&word.text) || scope.expanded_binding(word, &word.text).known().is_none()
            {
                continue;
            }
            if functions {
                if !scope.defining {
                    self.functions.remove(&word.text);
                }
            } else {
                let prefix = format!("{}[", word.text);
                scope
                    .bindings
                    .retain(|name, _| name != &word.text && !name.starts_with(&prefix));
            }
        }
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
        let exported = program == "export" && !argv.iter().any(|word| word == "-n");
        let arithmetic = argv
            .iter()
            .skip(1)
            .filter_map(|word| {
                let flag = word
                    .strip_prefix('-')
                    .map(|value| (value, true))
                    .or_else(|| word.strip_prefix('+').map(|value| (value, false)))?;
                flag.0.contains('i').then_some(flag.1)
            })
            .next_back();
        if in_function && (program == "export" || argv.iter().skip(1).any(|w| w.starts_with('-'))) {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
        }
        for word in &argv[1..] {
            if let Some((name, value)) = assignment(&word.text) {
                if local {
                    scope.local(name);
                }
                let mut values = vec![scope.expanded_binding(word, value)];
                if top_local {
                    let prior = scope
                        .bindings
                        .get(name)
                        .map_or_else(|| vec![BindingValue::Undetermined], |b| b.values.clone());
                    for value in prior {
                        if !values.contains(&value) {
                            values.push(value);
                        }
                    }
                    if values.contains(&BindingValue::Undetermined) {
                        self.output.gap(CoverageGap::UnsupportedShellSyntax);
                    }
                }
                scope.assign(name.into(), values);
                if program == "export"
                    && let Some(binding) = scope.bindings.get_mut(name)
                {
                    binding.exported = exported;
                }
            } else if program == "export" && identifier(&word.text) {
                scope
                    .bindings
                    .entry(word.text.clone())
                    .or_insert_with(|| Binding {
                        values: vec![BindingValue::RuntimeUnknown(None)],
                        exported,
                        arithmetic: false,
                    })
                    .exported = exported;
            } else if local && identifier(&word.text) {
                let mut values = scope
                    .bindings
                    .get(&word.text)
                    .map_or_else(Vec::new, |binding| binding.values.clone());
                let unset = BindingValue::RuntimeUnknown(Some(String::new()));
                if !values.contains(&unset) {
                    values.push(unset);
                }
                scope.local(&word.text);
                scope.assign(word.text.clone(), values);
            }
            if let Some(arithmetic) = arithmetic {
                let name = assignment(&word.text).map_or(word.text.as_str(), |(name, _)| name);
                if identifier(name) {
                    scope
                        .bindings
                        .entry(name.into())
                        .or_insert_with(|| Binding {
                            values: vec![BindingValue::RuntimeUnknown(None)],
                            exported: false,
                            arithmetic: false,
                        })
                        .arithmetic = arithmetic;
                }
            }
        }
    }
    fn positionals(&mut self, argv: &[crate::record::Word], scope: &mut Scope) {
        let prior = scope
            .bindings
            .keys()
            .filter(|name| name.parse::<usize>().is_ok())
            .cloned()
            .collect::<Vec<_>>();
        for name in prior {
            scope.local(&name);
            scope.bindings.remove(&name);
        }
        for (index, word) in argv.iter().enumerate() {
            let name = (index + 1).to_string();
            scope.local(&name);
            scope.assign(
                name,
                vec![if word.expands {
                    BindingValue::RuntimeUnknown(None)
                } else {
                    BindingValue::Known(word.text.clone())
                }],
            );
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
                    environment: Vec::new(),
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
                &[],
            );
        }
        Ok(())
    }
    fn emit(
        &mut self,
        mut command: Command,
        scope: &Scope,
        environment: &[super::argv::EnvironmentChange],
    ) -> Command {
        if let Some(index) = command.program {
            for (name, binding) in scope
                .bindings
                .iter()
                .filter(|(name, _)| matches!(name.as_str(), "GIT_DIR" | "GIT_WORK_TREE"))
            {
                let prefix = command.argv[..index].iter().any(|word| {
                    matches!(word.role, Role::Assign | Role::Precommand)
                        && assignment(&word.text).is_some_and(|(key, _)| key == name)
                });
                if binding.exported || prefix {
                    for value in &binding.values {
                        if let Some(text) = value.lexical() {
                            let mut word = crate::record::Word::literal(text.clone());
                            // Assignment expansion has already consumed any unquoted tilde.
                            word.raw = format!("'{text}'");
                            word.expands = matches!(value, BindingValue::RuntimeDerived(_));
                            command.environment.push((name.clone(), word));
                        }
                    }
                }
            }
        }
        // D48b applies the wrapper's effective environment before the Git owner
        // sees it; a child shell inherits the same values, including overrides.
        for change in environment {
            match change {
                super::argv::EnvironmentChange::Clear => command.environment.clear(),
                super::argv::EnvironmentChange::Unset(name) => {
                    command.environment.retain(|(key, _)| key != name)
                }
                super::argv::EnvironmentChange::Set(name, value)
                    if matches!(name.as_str(), "GIT_DIR" | "GIT_WORK_TREE") =>
                {
                    command.environment.retain(|(key, _)| key != name);
                    let mut value = value.clone();
                    if super::lexer::initial_quote(&value.raw) == super::lexer::Quote::Unquoted {
                        value.text = crate::filesystem::expand_home(
                            &value.text,
                            self.frontend.host.home,
                            self.frontend.host.user,
                        );
                        value.value = value.text.clone();
                    }
                    command.environment.push((name.clone(), value));
                }
                _ => {}
            }
        }
        if command.argv.iter().flat_map(|word| &word.vars).any(|name| {
            scope.bindings.get(name).is_some_and(|binding| {
                binding.values.iter().any(|value| {
                    matches!(
                        value,
                        BindingValue::RuntimeUnknown(_) | BindingValue::RuntimeDerived(_)
                    )
                })
            })
        }) && crate::targets::infer(&command, &command.cwd, self.frontend.host)
            .targets
            .iter()
            .any(|target| target.expands)
        {
            self.output.gap(CoverageGap::UnresolvedTarget);
        }
        if let Some(gap) = &scope.directory.gap {
            // Only overflow pays for this second inference. A different cwd exposes
            // target dependencies without inventing a separate adapter role table.
            let effects = crate::targets::infer(&command, &command.cwd, self.frontend.host);
            let relocated = crate::targets::infer(&command, "/", self.frontend.host);
            if command.argv.iter().any(|word| word.pwd)
                || effects.targets != relocated.targets
                || !effects.code.is_empty()
                || !effects.inline.is_empty()
            {
                self.output.gap(gap.clone());
            }
        }
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
            command.stdin = match super::argv::stdin_kind(&command) {
                Stdin::None => Stdin::Data(data),
                kind => kind,
            };
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
        command
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
        let operand = args
            .iter()
            .position(|w| !w.starts_with('-') || program == "cd" && w == "-");
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
        let oldpwd = program == "cd" && target == "-";
        if oldpwd {
            targets.clear();
            if let Some(binding) = scope.bindings.get("OLDPWD") {
                for value in &binding.values {
                    match value {
                        BindingValue::Known(value) => targets.push(value.clone()),
                        BindingValue::Undetermined => {
                            self.output.gap(CoverageGap::UnsupportedShellSyntax);
                        }
                        BindingValue::RuntimeUnknown(_) | BindingValue::RuntimeDerived(_) => {}
                    }
                }
            }
            if targets.is_empty() {
                return;
            }
        } else if program == "cd"
            && let Some(operand) = operand
            && args.len() == operand + 2
            && !args[operand].expands
            && !args[operand + 1].expands
        {
            let mut readings = std::iter::once(scope.directory.current.render())
                .chain(
                    scope
                        .directory
                        .alternatives
                        .iter()
                        .map(cwd::CwdPath::render),
                )
                .collect::<Vec<_>>();
            if let Some(binding) = scope.bindings.get("PWD") {
                for value in &binding.values {
                    match value {
                        BindingValue::Known(value) => readings.push(value.clone()),
                        BindingValue::Undetermined => {
                            self.output.gap(CoverageGap::UnsupportedShellSyntax);
                        }
                        BindingValue::RuntimeUnknown(_) | BindingValue::RuntimeDerived(_) => {}
                    }
                }
            }
            for reading in readings {
                if reading.contains(&target) {
                    let replaced = reading.replacen(&target, &args[operand + 1].text, 1);
                    if !targets.contains(&replaced) {
                        targets.push(replaced);
                    }
                }
            }
        }
        if operand.is_some()
            && !oldpwd
            && !target.starts_with(['/', '.'])
            && let Some(binding) = scope.bindings.get("CDPATH")
        {
            for value in &binding.values {
                if let BindingValue::Known(value) = value {
                    for entry in value.split(':').filter(|entry| !entry.is_empty()) {
                        let path = format!("{entry}/{target}");
                        if !targets.contains(&path) {
                            targets.push(path);
                        }
                    }
                } else if value == &BindingValue::Undetermined {
                    self.output.gap(CoverageGap::UnsupportedShellSyntax);
                }
            }
        }
        scope.assign(
            "OLDPWD".into(),
            std::iter::once(&scope.directory.current)
                .chain(&scope.directory.alternatives)
                .map(|path| BindingValue::Known(path.render()))
                .collect(),
        );
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
        super::expand_scoped(raw, scope, self, depth, true)
    }
    pub fn isolated_source(
        &mut self,
        source: &str,
        scope: &Scope,
        depth: usize,
    ) -> Result<(), CheckError> {
        let functions = self.functions.clone();
        let result = self.source(source, &mut scope.isolated(), depth);
        self.functions = functions;
        result
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
        let evaluation = super::arithmetic::evaluate(expression, &scope.values())?;
        if evaluation.bounded {
            self.output.gap(CoverageGap::InspectionBudget);
        }
        if evaluation.names.iter().any(|name| {
            scope
                .bindings
                .get(name)
                .is_some_and(|b| b.values.contains(&BindingValue::Undetermined))
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
    pub(super) fn armed_reference(
        &mut self,
        name: &str,
        scope: &mut Scope,
        depth: usize,
    ) -> Result<(), CheckError> {
        let values = scope.candidates().remove(name).unwrap_or_default();
        if values.is_empty() {
            return Ok(());
        }
        let mut sources = Vec::new();
        for value in &values {
            let BindingValue::Known(value) = value else {
                if value == &BindingValue::Undetermined
                    || matches!(value, BindingValue::RuntimeDerived(text)
                        if !matches!(super::arithmetic::armed(text), super::arithmetic::Arming::Inert))
                {
                    self.output.gap(CoverageGap::UnsupportedShellSyntax);
                }
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
                self.isolated_source(&source, scope, depth + 1)?;
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
pub(super) fn assignment(raw: &str) -> Option<(&str, &str)> {
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
            .map(|v| BindingValue::Known(v.as_str().unwrap().into()))
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
        outer.assign("D".into(), vec![BindingValue::Known("public".into())]);
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
        scope.assign("A".into(), vec![BindingValue::Undetermined]);
        let result = observation(
            data["sources"]["unknown_arithmetic"].as_str().unwrap(),
            &mut scope,
        );
        assert!(result.gaps.contains(&CoverageGap::UnsupportedShellSyntax));
    }
    #[test]
    fn runtime_glob_propagation_stays_unknown() {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/rust-m2-1.json")).unwrap();
        let row = data["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "loop-unknown-glob")
            .unwrap();
        let mut scope = Scope::new("/h", "/p");
        let result = observation(row["source"].as_str().unwrap(), &mut scope);
        assert!(
            scope.bindings["D"]
                .values
                .contains(&BindingValue::RuntimeDerived("public*".into())),
            "{:?}: {result:?}",
            scope.bindings
        );
    }
    #[test]
    fn finite_loop_retains_all_directory_candidates() {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/rust-m2-1.json")).unwrap();
        let row = data["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "loop-cwd-up-three")
            .unwrap();
        let mut scope = Scope::new("/h", row["cwd"].as_str().unwrap());
        let result = observation(row["source"].as_str().unwrap(), &mut scope);
        assert!(
            scope
                .directory
                .alternatives
                .iter()
                .any(|path| path.render() == "/h/project")
        );
        assert_eq!(scope.directory.current.render(), "/h");
        assert!(
            !result.gaps.contains(&CoverageGap::InspectionBudget),
            "{result:?}"
        );
    }
    #[test]
    fn tracked_movement_updates_oldpwd_binding() {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/rust-m2-1.json")).unwrap();
        let row = data["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "oldpwd-metadata-control")
            .unwrap();
        let mut scope = Scope::new("/h", "/h/project");
        observation(row["source"].as_str().unwrap(), &mut scope);
        assert!(
            scope.bindings["OLDPWD"]
                .values
                .contains(&BindingValue::Known("/h/public".into()))
        );
        assert_eq!(scope.directory.current.render(), "/h/project");
    }
    #[test]
    fn binding_values_and_exit_snapshots_are_bounded() {
        let mut outer = Scope::new("/h", "/p");
        outer.loops.push(Vec::new());
        let branches = (0..513)
            .map(|n| {
                let mut branch = outer.clone();
                branch.assign("A".into(), vec![BindingValue::Known(n.to_string())]);
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
            cwd::bounded(&current, paths[..16].to_vec(), "/h").0,
            paths[..16]
        );
        assert_eq!(
            cwd::bounded(&current, paths, "/h").0,
            [
                cwd::CwdPath::Logical("/h".into()),
                cwd::CwdPath::Logical("/h/Library".into())
            ]
        );
    }
}
