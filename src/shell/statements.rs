use super::cwd::{self, Directory};
use super::{Expanded, Frontend, Observation, Operator, RawWord, Statement, WordSyntax};
use crate::{
    CheckError, CoverageGap,
    limits::MAX_NESTING,
    record::{Command, Role, Stdin},
};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod loop_cost {
    #[test]
    fn unconditional_literal_append_keeps_exact_string() {
        for width in [4, 8, 16] {
            let items = (0..width)
                .map(|n| format!("public{n}"))
                .collect::<Vec<_>>()
                .join(" ");
            let source = format!("M=prefix; for f in {items}; do M=\"$M $f\"; done; echo \"$M\"");
            let output =
                crate::shell::observe(&source, crate::shell::Arm::Brush, "/h", "/h/p", true)
                    .unwrap();
            let echo = output.script.commands.last().unwrap();
            assert_eq!(echo.argv[1].text, format!("prefix {items}"));
            assert!(!echo.argv[1].cardinality_unknown);
            assert!(!echo.argv[1].expands);
        }
    }

    #[test]
    fn conditional_literal_accumulation_work_grows_polynomially() {
        for bound in [false, true] {
            let count = |width| {
                let items = (0..width)
                    .map(|n| format!("public{n}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let header = if bound {
                    format!("REQ='{items}'; for f in $REQ")
                } else {
                    format!("for f in {items}")
                };
                let source = format!(
                    "M=prefix; {header}; do test -f public || M=\"$M $f\"; done; echo \"[$M]\""
                );
                let output = crate::shell::observe(
                    &source,
                    crate::shell::Arm::Brush,
                    "/synthetic/home",
                    "/synthetic/project",
                    true,
                )
                .unwrap();
                assert!(
                    !output.gaps.contains(&crate::CoverageGap::InspectionBudget),
                    "bound={bound} width={width}: {:?}",
                    output.gaps
                );
                (
                    output.statement_visits,
                    output.candidate_pairs,
                    output.script.commands.len(),
                )
            };
            let counts = [4, 8, 16].map(count);
            println!("conditional accumulation bound={bound}: {counts:?}");
            for (small, large) in counts.iter().zip(counts.iter().skip(1)) {
                assert!(
                    small.0 > 0
                        && large.0 <= small.0 * 5
                        && large.1 <= small.1 * 5
                        && large.2 <= small.2 * 5,
                    "bound={bound}: {counts:?}"
                );
            }
        }
    }
    #[test]
    fn unknown_fragment_repetition_converges_before_branching() {
        let count = |width| {
            let items = (0..width)
                .map(|n| format!("public{n}"))
                .collect::<Vec<_>>()
                .join(" ");
            let source = format!(
                "for f in {items}; do Q=\"\"; while read id; do Q=\"${{Q}}&x=${{id}}\"; done < \"$f\"; for c in true false; do curl \"https://example.test/?c=${{c}}${{Q}}\" -o public.json; done; done"
            );
            let output = crate::shell::observe(
                &source,
                crate::shell::Arm::Brush,
                "/synthetic/home",
                "/synthetic/project",
                true,
            )
            .unwrap();
            assert!(
                !output.gaps.contains(&crate::CoverageGap::InspectionBudget),
                "width={width}: {:?}",
                output.gaps
            );
            (output.statement_visits, output.script.commands.len())
        };
        let counts = [2, 4, 8].map(count);
        println!("unknown repetition work={counts:?}");
        for (small, large) in counts.iter().zip(counts.iter().skip(1)) {
            assert!(
                small.0 > 0 && large.0 <= small.0 * 3 && large.1 <= small.1 * 3,
                "{counts:?}"
            );
        }
    }
    #[test]
    fn repeated_unknown_hits_converge_before_the_record_budget() {
        for width in [2, 4] {
            let items = (0..width)
                .map(|n| format!("public{n}"))
                .collect::<Vec<_>>()
                .join(" ");
            let source = format!(
                "hit=\"\"; for p in {items}; do for c in $(printf public); do [ \"$(printf public)\" = public ] && hit=\"$hit ${{c:0:7}}:$p\"; done; done; echo \"$hit\""
            );
            let observation = crate::shell::observe(
                &source,
                crate::shell::Arm::Brush,
                "/synthetic/home",
                "/synthetic/project",
                true,
            )
            .unwrap();
            assert!(
                !observation
                    .gaps
                    .contains(&crate::CoverageGap::InspectionBudget)
            );
            assert!(
                observation.source_entries <= width * 8,
                "width={width}, recursive sources={}",
                observation.source_entries
            );
        }
    }
    #[test]
    fn loop_directory_work_grows_polynomially() {
        let cost = |width| {
            let items = (0..width)
                .map(|n| format!("public{n}"))
                .collect::<Vec<_>>()
                .join(" ");
            let source =
                format!("for r in a b c d; do for b in {items}; do cd $r/$b && ls; done; done");
            let observation = crate::shell::observe(
                &source,
                crate::shell::Arm::Brush,
                "/synthetic/home",
                "/synthetic/project",
                true,
            )
            .unwrap();
            println!(
                "width={width}, cwd={}, gaps={:?}",
                observation.cwd_candidates, observation.gaps
            );
            assert!(
                !observation
                    .gaps
                    .contains(&crate::CoverageGap::InspectionBudget)
            );
            observation.cwd_candidates
        };
        let small = cost(5);
        let large = cost(10);
        println!("cwd candidates {small} -> {large}");
        assert!(small > 0 && large <= small * 4);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum BindingValue {
    Known(String),
    // Keep the uncommon repetition payload off the recursive evaluator stack.
    RepeatedFields(Box<LiteralRepetition>),
    // The lexical representative preserves pre-M2 target inference; it is
    // never evidence of the runtime value or its arithmetic contents.
    RuntimeUnknown(Option<String>),
    // Derived text must carry its runtime uncertainty through substitution.
    RuntimeDerived(String),
    ShellMatches(String),
    ShellDerived(String),
    Undetermined,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LiteralRepetition {
    pub prefix: String,
    pub alternatives: Vec<String>,
    pub suffix: String,
    pub may_be_empty: bool,
}

impl LiteralRepetition {
    pub fn projections(&self) -> Vec<String> {
        let mut values = Vec::new();
        if self.may_be_empty {
            values.push(format!("{}{}", self.prefix, self.suffix));
        }
        for alternative in &self.alternatives {
            values.push(format!("{}{alternative}{}", self.prefix, self.suffix));
            if !self.suffix.is_empty() {
                // Only the final field receives a copied suffix; earlier fields
                // still reach the consumer with their original identities.
                values.push(format!("{}{alternative}", self.prefix));
            }
        }
        values
    }
}

impl BindingValue {
    fn lexical(&self) -> Option<&String> {
        match self {
            Self::Known(value)
            | Self::RuntimeUnknown(Some(value))
            | Self::RuntimeDerived(value)
            | Self::ShellMatches(value)
            | Self::ShellDerived(value) => Some(value),
            Self::RepeatedFields(_) | Self::RuntimeUnknown(None) | Self::Undetermined => None,
        }
    }
    pub fn known(&self) -> Option<&String> {
        match self {
            Self::Known(value) => Some(value),
            Self::RepeatedFields(_)
            | Self::RuntimeUnknown(_)
            | Self::RuntimeDerived(_)
            | Self::ShellMatches(_)
            | Self::ShellDerived(_)
            | Self::Undetermined => None,
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
    summarizing_loop: bool,
    bounded_loop: bool,
    conditional_append: bool,
    piped: bool,
    pipeline_input: Option<Vec<String>>,
    relative_glob_moves: usize,
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
            summarizing_loop: false,
            bounded_loop: false,
            conditional_append: false,
            piped: false,
            pipeline_input: None,
            relative_glob_moves: 0,
        }
    }
    pub fn isolated(&self) -> Self {
        let mut child = self.clone();
        child.returns.clear();
        child.loops.clear();
        child.summarizing_loop = false;
        child.bounded_loop = false;
        child.conditional_append = false;
        child.pipeline_input = None;
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
    fn repeated_word(&self, word: &crate::record::Word) -> bool {
        word.vars.iter().any(|name| {
            self.bindings.get(name).is_some_and(|binding| {
                binding
                    .values
                    .iter()
                    .any(|value| matches!(value, BindingValue::RepeatedFields(_)))
            })
        })
    }
    pub fn positional_words(&self) -> Option<Vec<crate::record::Word>> {
        let count = self.bindings.get("#")?;
        if count.values.len() != 1 {
            return None;
        }
        if count.values[0].known().is_none() && self.defining {
            let mut word = crate::record::Word::literal("${@}".into());
            word.expands = true;
            word.runtime_unknown = true;
            word.vars.push("@".into());
            return Some(vec![word]);
        }
        let count = count.values[0].known()?.parse::<usize>().ok()?;
        let mut words = Vec::new();
        for index in 1..=count {
            let name = index.to_string();
            let binding = self.bindings.get(&name)?;
            if binding.values.len() != 1 {
                return None;
            }
            let value = &binding.values[0];
            let mut word = crate::record::Word::literal(
                value
                    .lexical()
                    .cloned()
                    .unwrap_or_else(|| format!("${{{name}}}")),
            );
            word.vars.push(name);
            word.expands = value.known().is_none();
            word.runtime_unknown = matches!(
                value,
                BindingValue::RuntimeUnknown(_) | BindingValue::RuntimeDerived(_)
            );
            word.shell_matches = matches!(
                value,
                BindingValue::ShellMatches(_) | BindingValue::ShellDerived(_)
            );
            words.push(word);
        }
        Some(words)
    }
    fn expanded_binding(&self, word: &crate::record::Word, text: &str) -> BindingValue {
        let raw = if text != word.text {
            assignment(&word.raw).map_or(word.raw.as_str(), |(_, value)| value)
        } else {
            &word.raw
        };
        for name in &word.vars {
            if let Some(binding) = self.bindings.get(name) {
                for value in &binding.values {
                    if let BindingValue::RepeatedFields(repeated) = value
                        && let Some((prefix, suffix, _)) =
                            super::words::parameter_affixes(raw, name)
                        && word.binding_candidates.get(name).is_some_and(|value| {
                            repeated.projections().contains(value)
                                && text == format!("{prefix}{value}{suffix}")
                        })
                    {
                        let mut repeated = repeated.clone();
                        repeated.prefix = format!("{prefix}{}", repeated.prefix);
                        repeated.suffix.push_str(&suffix);
                        return BindingValue::RepeatedFields(repeated);
                    }
                }
            }
        }
        if word.cardinality_unknown
            && super::words::parameter_affixes(raw, word.vars.first().map_or("", String::as_str))
                .is_none()
        {
            return BindingValue::RuntimeDerived(text.into());
        }
        if word.shell_matches {
            return if word.globs || word.expands {
                BindingValue::ShellDerived(text.into())
            } else {
                BindingValue::ShellMatches(text.into())
            };
        }
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
            // Repetition of unknown read fields is not new lexical evidence.
            // Widen only placeholder-only loop aggregates; any literal path,
            // arithmetic syntax or code keeps its full candidate text.
            if !self.loops.is_empty() {
                let mut fields = Vec::new();
                let mut placeholders = true;
                for field in text.split_whitespace() {
                    let name = field
                        .strip_prefix("${")
                        .and_then(|name| name.strip_suffix('}'))
                        .or_else(|| field.strip_prefix('$'));
                    placeholders &= name.is_some_and(identifier);
                    if !fields.contains(&field) {
                        fields.push(field);
                    }
                }
                if placeholders && !fields.is_empty() {
                    return BindingValue::RuntimeDerived(fields.join(" "));
                }
            }
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
            self.relative_glob_moves = self.relative_glob_moves.max(branch.relative_glob_moves);
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
            if let BindingValue::RepeatedFields(repetition) = &value
                && let Some(BindingValue::RepeatedFields(existing)) = joined.iter_mut().find(|v| {
                    matches!(
                        v,
                        BindingValue::RepeatedFields(other)
                            if other.prefix == repetition.prefix && other.suffix == repetition.suffix
                    )
                })
            {
                existing.may_be_empty |= repetition.may_be_empty;
                for alternative in &repetition.alternatives {
                    if !existing.alternatives.contains(alternative) {
                        if existing.alternatives.len() == 512 {
                            bounded = true;
                        } else {
                            existing.alternatives.push(alternative.clone());
                        }
                    }
                }
                continue;
            }
            if !joined.contains(&value) {
                if joined.len() == 512 {
                    bounded = true;
                } else {
                    joined.push(value);
                }
            }
        }
    }
    let mut empty = Vec::new();
    for value in &joined {
        if let BindingValue::Known(text) = value
            && joined.iter().any(|other| {
                matches!(other, BindingValue::RepeatedFields(repetition)
                if text == &format!("{}{}", repetition.prefix, repetition.suffix))
            })
        {
            empty.push(text.clone());
        }
    }
    for value in &mut joined {
        if let BindingValue::RepeatedFields(repetition) = value
            && empty.contains(&format!("{}{}", repetition.prefix, repetition.suffix))
        {
            repetition.may_be_empty = true;
        }
    }
    joined.retain(|value| !matches!(value, BindingValue::Known(text) if empty.contains(text)));
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

fn widen_runtime_repetition(prior: &Scope, next: &mut Scope) {
    for (name, binding) in &mut next.bindings {
        let Some(before) = prior.bindings.get(name) else {
            continue;
        };
        for value in &mut binding.values {
            if !matches!(
                value,
                BindingValue::RuntimeUnknown(_)
                    | BindingValue::RuntimeDerived(_)
                    | BindingValue::ShellDerived(_)
            ) {
                continue;
            }
            let Some(text) = value.lexical() else {
                continue;
            };
            for old in &before.values {
                if old.known().is_some() {
                    continue;
                }
                let Some(old) = old.lexical() else {
                    continue;
                };
                let Some(tail) = text.strip_prefix(old).filter(|tail| !tail.is_empty()) else {
                    continue;
                };
                if let Some(prefix) = old.strip_suffix(tail) {
                    // A repeated unknown fragment denotes arbitrary repetitions,
                    // not additional runtime evidence. Keep its fixed prefix and
                    // suffix; pathname matching must cover the widened middle.
                    *value = if tail.split('/').any(|part| part == "..") {
                        // Repeated parent traversal can escape the fixed prefix.
                        // A pathname pattern cannot represent normalization here.
                        BindingValue::Undetermined
                    } else if tail.contains('/') {
                        let prefix = prefix.strip_suffix("*/**/*").unwrap_or(prefix);
                        BindingValue::ShellDerived(format!("{prefix}*/**/*{tail}"))
                    } else {
                        BindingValue::ShellDerived(format!(
                            "{}*{tail}",
                            prefix.trim_end_matches('*')
                        ))
                    };
                    break;
                }
            }
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
    pub(super) deadline: Option<std::time::Instant>,
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
            deadline: None,
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
            crate::check_deadline(self.deadline)?;
            #[cfg(test)]
            {
                self.output.statement_visits += 1;
            }
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
            scope.directory.relative_growth = scope
                .directory
                .relative_growth
                .max(branch.directory.relative_growth);
            scope.directory.gap = scope.directory.gap.take().or(branch.directory.gap.clone());
        }
        scope
            .directory
            .merge(candidates.into_iter(), self.frontend.host.home);
    }
    fn literal_accumulation(
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
        let Some(tail) = super::words::without_leading_parameter(&raw.raw, name) else {
            return Ok(None);
        };
        let variables = scope.contexts();
        let cwd = scope.directory.current.render();
        let preview = super::words::expand(
            &tail,
            &raw.syntax,
            &super::words::ExpansionContext {
                variables: &variables,
                runtime_variables: &BTreeSet::new(),
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
        let prior = scope.bindings.get(name).map_or_else(
            || vec![BindingValue::Known(String::new())],
            |binding| binding.values.clone(),
        );
        let mut repeated = Vec::new();
        for value in prior {
            let value = match value {
                BindingValue::Known(prefix)
                    if matches!(
                        super::arithmetic::armed(&prefix),
                        super::arithmetic::Arming::Inert
                    ) =>
                {
                    LiteralRepetition {
                        prefix,
                        alternatives: Vec::new(),
                        suffix: String::new(),
                        may_be_empty: false,
                    }
                }
                BindingValue::RepeatedFields(mut value) if value.suffix.is_empty() => {
                    value.may_be_empty = false;
                    *value
                }
                _ => return Ok(None),
            };
            repeated.push(value);
        }
        let tails = super::expand_scoped(
            &RawWord {
                raw: tail,
                syntax: raw.syntax.clone(),
                expansions: Vec::new(),
            },
            scope,
            self,
            depth,
            false,
        )?;
        for tail in tails {
            let tail = tail.word.text;
            // Field boundaries keep every literal's resource identity independent
            // of repetition count. Contiguous bytes keep the sequential owner.
            if !tail.starts_with([' ', '\t', '\n'])
                || !matches!(
                    super::arithmetic::armed(&tail),
                    super::arithmetic::Arming::Inert
                )
            {
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
    fn statement(
        &mut self,
        statement: &Statement,
        scope: &mut Scope,
        depth: usize,
        source_id: usize,
        nested: bool,
    ) -> Result<(), CheckError> {
        // Keep compound temporaries off the recursively entered command frame.
        #[cfg(test)]
        {
            self.output.failure_copies += scope.directory.failures.as_ref().map_or(0, Vec::len);
            self.output.cwd_candidates += scope.directory.alternatives.len() + 1;
        }
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
            let accumulated = self.literal_accumulation(name, raw, &mut assignment_scope, depth)?;
            let values = if accumulated.is_some() {
                Vec::new()
            } else {
                super::expand_scoped(raw, &mut assignment_scope, self, depth, observe_bindings)?
            };
            for value in &values {
                self.armed_references(&value.word.text, &mut assignment_scope, depth)?;
            }
            let mut binding = accumulated.unwrap_or_else(|| {
                values
                    .iter()
                    .map(|v| assignment_scope.expanded_binding(&v.word, &v.word.text))
                    .collect::<Vec<_>>()
            });
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
                    if !expanded.positional {
                        choices.push(vec![expanded.word]);
                    }
                }
            }
            choices.dedup();
            arguments.push(choices);
        }
        let mut initial = prefixes.clone();
        for choices in &arguments {
            if let Some(choice) = choices.first() {
                initial.extend(choice.clone());
            }
        }
        let resolution = super::argv::resolve(
            &mut initial,
            &scope.directory.current.render(),
            self.frontend.host,
        );
        let independent = prefixes.is_empty()
            && !scope.piped
            && pipeline.is_none()
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
                    argv: initial.clone(),
                    program: resolution.program,
                    cwd: resolution.cwd,
                    wrappers: resolution.wrappers,
                    shell: resolution.shell,
                    function: false,
                    environment: Vec::new(),
                    redirects: Vec::new(),
                    flags: Vec::new(),
                    items: None,
                    stdin: Stdin::None,
                    pipeline: None,
                    nested,
                },
                &scope.directory.current.render(),
                self.frontend.host,
            )
            .independent_arguments;
        let mut alternatives = if independent {
            vec![initial.clone()]
        } else {
            vec![prefixes]
        };
        for (index, choices) in arguments.iter().enumerate() {
            if independent {
                // Every candidate still reaches its owner; independent fields
                // need their union, not every Cartesian combination.
                for choice in choices.iter().skip(1) {
                    let mut candidate = initial.clone();
                    candidate[index] = choice[0].clone();
                    alternatives.push(candidate);
                }
                continue;
            }
            let mut next = Vec::new();
            for previous in &alternatives {
                for choice in choices {
                    #[cfg(test)]
                    {
                        self.output.candidate_pairs += 1;
                    }
                    if !compatible_arguments(previous, choice) {
                        continue;
                    }
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
                let words = if scope.repeated_word(&expanded.word) {
                    expanded.split
                } else {
                    vec![expanded.word]
                };
                targets.extend(
                    words
                        .into_iter()
                        .map(|word| crate::record::Redirect::from_word(word, redirect.direction)),
                );
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
                for (position, word) in argv[index + 1..].iter().enumerate() {
                    let name_operand = resolved.shell
                        && !self.functions.contains_key(&argv[index].text)
                        && identifier(&word.text)
                        && match argv[index].text.as_str() {
                            "unset" | "export" | "read" => true,
                            "printf" => position == 1 && argv[index + 1] == "-v",
                            "local" | "declare" | "typeset" => !scope.frames.is_empty(),
                            _ => false,
                        };
                    if !name_operand {
                        self.armed_word(word, scope, depth)?;
                    }
                }
            }
            if let Some(index) = program.filter(|i| {
                !self.functions.contains_key(&argv[*i].text)
                    && (resolved.shell
                        || argv[*i] == "eval" && resolved.wrappers.iter().any(|w| w == "command"))
            }) {
                if argv[index].expands && !self.functions.is_empty() && !scope.defining {
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
                    "shift" => {
                        if scope.defining
                            && scope.bindings.get("#").is_some_and(|binding| {
                                binding.values.iter().any(|value| value.known().is_none())
                            })
                        {
                            continue;
                        }
                        let amount = if argv.len() == index + 1 {
                            Some(1)
                        } else {
                            argv.get(index + 1)
                                .filter(|word| !word.expands)
                                .and_then(|word| word.parse::<usize>().ok())
                        };
                        if let (Some(amount), Some(arguments)) = (amount, scope.positional_words())
                        {
                            if amount <= arguments.len() {
                                self.positionals(&arguments[amount..], scope);
                            }
                        } else if scope.bindings.contains_key("#") {
                            self.output.gap(CoverageGap::UnsupportedShellSyntax);
                        } else {
                            self.output.gap(CoverageGap::UnresolvedTarget);
                        }
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
                                let binding = if values[1].cardinality_unknown {
                                    scope.expanded_binding(&values[1], &values[1].text)
                                } else {
                                    BindingValue::Known(values[1].text.clone())
                                };
                                scope.assign(name.text.clone(), vec![binding]);
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
                        if argv[index + 1..]
                            .iter()
                            .any(|word| word.expands || word.cardinality_unknown)
                        {
                            self.output.gap(
                                if argv[index + 1..].iter().any(|word| {
                                    word.cardinality_unknown
                                        || word.expands
                                            && !word.runtime_unknown
                                            && !word.vars.is_empty()
                                }) || !scope.frames.is_empty()
                                    || !prior.is_empty()
                                {
                                    CoverageGap::UnsupportedShellSyntax
                                } else {
                                    CoverageGap::UnresolvedTarget
                                },
                            );
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
                        self.read(&argv[index + 1..], &targets, scope);
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
                            (binding.exported || prior.contains_key(*name))
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
                let streams = command
                    .redirects
                    .iter()
                    .filter(|redirect| redirect.direction == crate::record::Direction::In)
                    .filter_map(|redirect| redirect.stream.as_deref())
                    .chain(crate::targets::list_file_sources(&command));
                for stream in streams {
                    if let crate::record::StreamOutput::Known(outputs) = stream {
                        for output in outputs {
                            for input in super::pipeline::xargs_here_input(&command, output) {
                                self.source(
                                    &input.source,
                                    &mut Scope::new(self.frontend.host.home, &input.cwd),
                                    depth + 1,
                                )?;
                            }
                        }
                    }
                }
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
            Statement::Redirected(redirects, body) => {
                let prior = scope.pipeline_input.take();
                let mut targets = Vec::new();
                for redirect in redirects {
                    for expanded in self.expand(&redirect.target, scope, depth)? {
                        if redirect.direction == crate::record::Direction::In {
                            scope.pipeline_input = match expanded.word.stream.as_deref() {
                                Some(crate::record::StreamOutput::Known(output)) => {
                                    Some(output.clone())
                                }
                                _ => None,
                            };
                        } else if matches!(
                            redirect.direction,
                            crate::record::Direction::Heredoc
                                | crate::record::Direction::Herestring
                        ) && !expanded.word.expands
                        {
                            scope.pipeline_input = Some(vec![expanded.word.text.clone()]);
                        }
                        let words = if scope.repeated_word(&expanded.word) {
                            expanded.split
                        } else {
                            vec![expanded.word]
                        };
                        targets.extend(words.into_iter().map(|word| {
                            crate::record::Redirect::from_word(word, redirect.direction)
                        }));
                    }
                }
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
                self.run(body, scope, depth + 1, source_id, nested)?;
                scope.pipeline_input = prior;
            }
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
                if scope.isolated || scope.conditional_definition {
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
                    && scope.frames.is_empty()
                {
                    self.output.gap(CoverageGap::UnsupportedShellSyntax);
                }
                let mut inner = scope.isolated();
                // Body inspection must not publish nested definitions or create
                // an isolated-shell refusal for an ordinary function frame.
                inner.isolated = scope.isolated;
                inner.defining = true;
                inner.enter_function();
                // Definitions have unknown argv, not an invocation with no arguments.
                self.positionals(&[], &mut inner);
                inner.assign("#".into(), vec![BindingValue::RuntimeUnknown(None)]);
                let inserted = self.running.insert(name.clone());
                let functions = self.functions.clone();
                let result = self.run(body, &mut inner, depth + 1, source_id, nested);
                self.functions = functions;
                if inserted {
                    self.running.remove(name);
                }
                result?;
            }
            Statement::Binary(Operator::And | Operator::Or, _, _) => {
                self.logical_list(statement, scope, depth, source_id, nested)?;
            }
            Statement::Binary(Operator::Pipe, left, right) => {
                let before = scope.isolated();
                let start = self.output.script.commands.len();
                let mut producer = before.isolated();
                producer.piped = true;
                self.statement(left, &mut producer, depth, source_id, nested)?;
                let middle = self.output.script.commands.len();
                let mut rhs = before.isolated();
                rhs.piped = true;
                if let Statement::Command {
                    pipeline: Some(id), ..
                } = right.as_ref()
                {
                    rhs.pipeline_input = super::pipeline::read_input(
                        &self.output.script.commands[start..middle],
                        (source_id, *id),
                        |word| {
                            producer
                                .expanded_binding(word, &word.text)
                                .known()
                                .is_some()
                        },
                    );
                }
                self.statement(right, &mut rhs, depth, source_id, nested)?;
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
                let lhs = scope.clone();
                self.merge_bindings(scope, &[lhs, rhs.clone()]);
                self.merge_directories(scope, &[rhs]);
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
                yes.conditional_append = true;
                self.run(then, &mut yes, depth + 1, source_id, nested)?;
                let mut no = test.branch();
                no.conditional_append = true;
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
                    inner.conditional_append = true;
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
                let mut finite = true;
                let mut count = Some(0usize);
                for word in header {
                    let mut width = 0;
                    for expanded in self.expand(word, scope, depth)? {
                        finite &= expanded.word.vars.iter().all(|name| {
                            scope.bindings.get(name).is_none_or(|binding| {
                                binding.values.iter().all(|value| value.known().is_some())
                            })
                        });
                        literal &= word.expansions.is_empty()
                            && expanded.word.vars.is_empty()
                            && !expanded.word.expands
                            && !expanded.word.globs
                            && !expanded.tilde
                            && expanded.nested.is_empty()
                            && expanded.arithmetic.is_empty();
                        literal_values.push(expanded.word.text.clone());
                        width = width.max(expanded.split.len().max(1));
                        if expanded.word.expands
                            || expanded.word.globs
                            || expanded.word.cardinality_unknown
                        {
                            count = None;
                        }
                        values.extend(expanded.split.into_iter().map(|w| {
                            if w.expands {
                                BindingValue::RuntimeUnknown(None)
                            } else if w.globs {
                                BindingValue::ShellMatches(w.text)
                            } else {
                                BindingValue::Known(w.text)
                            }
                        }));
                        let unsplit = if expanded.word.expands {
                            BindingValue::RuntimeUnknown(None)
                        } else if expanded.word.globs {
                            BindingValue::ShellMatches(expanded.word.text)
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
                inner.bounded_loop = variable.is_some() && finite && count.is_some();
                inner.conditional_append = false;
                if !*empty
                    && count.is_some()
                    && !values.is_empty()
                    && finite
                    && let Some(variable) = variable
                    && self.loop_body_is_invariant(body, scope, variable)?
                {
                    let root = !scope.summarizing_loop;
                    inner.summarizing_loop = true;
                    inner.assign(variable.clone(), values);
                    let mut completed = 0;
                    loop {
                        let prior = inner.clone();
                        self.run(body, &mut inner, depth + 1, source_id, nested)?;
                        completed += 1;
                        if !root
                            || inner.bindings == prior.bindings
                            || completed >= count.unwrap_or(0)
                        {
                            break;
                        }
                        if self.inspected >= 512
                            || completed >= 512
                            || self.output.script.commands.len() > 512
                        {
                            self.output.gap(CoverageGap::InspectionBudget);
                            break;
                        }
                    }
                    inner.summarizing_loop = scope.summarizing_loop;
                    inner.bounded_loop = scope.bounded_loop;
                    inner.conditional_append = scope.conditional_append;
                    if literal {
                        if let Some(last) = literal_values.last() {
                            inner.assign(variable.clone(), vec![BindingValue::Known(last.clone())]);
                        }
                        inner.conditional_definition = scope.conditional_definition;
                        *scope = inner;
                    } else {
                        self.merge_directories(scope, &[before.clone(), inner.clone()]);
                        self.merge_bindings(scope, &[before, inner]);
                    }
                    return Ok(());
                }
                if literal
                    && literal_values.len() <= 512
                    && let Some(variable) = variable
                {
                    inner.loops.push(Vec::new());
                    for value in literal_values {
                        if inner.directory.relative_growth > before.directory.relative_growth
                            && inner.directory.current != before.directory.current
                        {
                            inner.directory.widen_loop(&before.directory);
                        }
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
                let carried_glob_cwd =
                    iterations.is_none() && inner.relative_glob_moves > before.relative_glob_moves;
                if carried_glob_cwd {
                    // Relative matches depend on the header cwd. Repeating a body
                    // that carries cwd cannot treat the pattern as a fixed pathname.
                    inner.directory = before.directory.clone();
                    inner.directory.gap = Some(CoverageGap::IdentityBound);
                    self.output.gap(CoverageGap::IdentityBound);
                }
                let mut branches = vec![before, inner.clone()];
                let mut completed = 1;
                let mut cwd_widened = false;
                while !carried_glob_cwd && iterations.is_none_or(|n| completed < n) {
                    if inner.directory.relative_growth > branches[0].directory.relative_growth
                        && (inner.directory.current != branches[0].directory.current
                            || iterations.is_none()
                                && inner.directory.alternatives
                                    != branches[0].directory.alternatives)
                    {
                        if iterations.is_none() {
                            if !cwd_widened {
                                inner.directory.widen_unknown_loop(&branches[0].directory);
                                cwd_widened = true;
                            }
                        } else {
                            inner.directory.widen_loop(&branches[0].directory);
                        }
                    }
                    let prior = inner.clone();
                    self.run(body, &mut inner, depth + 1, source_id, nested)?;
                    if iterations.is_none() {
                        widen_runtime_repetition(&prior, &mut inner);
                    }
                    completed += 1;
                    branches.push(inner.clone());
                    if inner.bindings == prior.bindings
                        && inner.directory.current == prior.directory.current
                        && inner.directory.alternatives == prior.directory.alternatives
                    {
                        break;
                    }
                    if self.inspected >= 512
                        || completed >= 512
                        || self.output.script.commands.len() > 512
                    {
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
    fn logical_list(
        &mut self,
        statement: &Statement,
        scope: &mut Scope,
        depth: usize,
        source_id: usize,
        nested: bool,
    ) -> Result<(), CheckError> {
        // The parser's left-associated list does not add syntactic nesting.
        // Preserve each branch continuation without recursive command frames.
        let mut frames = Vec::new();
        let mut current = statement;
        while let Statement::Binary(operator @ (Operator::And | Operator::Or), left, right) =
            current
        {
            let before = scope.isolated();
            let qualified = matches!(operator, Operator::And) && cwd::moved_on_success(left);
            let outer_failures = if qualified {
                let outer = scope.directory.failures.take();
                scope.directory.failures = Some(Vec::new());
                outer
            } else {
                None
            };
            let definition_on_success = matches!(operator, Operator::And)
                && (qualified || cwd::directory_success_guard(left));
            frames.push((
                operator,
                right.as_ref(),
                before,
                qualified,
                outer_failures,
                definition_on_success,
            ));
            current = left;
        }
        self.statement(current, scope, depth, source_id, nested)?;
        while let Some((
            operator,
            right,
            before,
            qualified,
            outer_failures,
            definition_on_success,
        )) = frames.pop()
        {
            let left_exit = scope.clone();
            let mut after = scope.branch();
            after.conditional_append = true;
            // Directory-command success does not require a statically known
            // destination. Preserve an enclosing if/case uncertainty.
            if definition_on_success {
                after.conditional_definition = scope.conditional_definition;
            }
            if qualified && !matches!(right, Statement::Command { .. }) {
                after.directory.failures = None;
            }
            self.statement(right, &mut after, depth, source_id, nested)?;
            if matches!(operator, Operator::And) {
                let mut failures = left_exit.directory.failures.clone().unwrap_or_default();
                cwd::extend_unique(
                    &mut failures,
                    after.directory.failures.clone().unwrap_or_default(),
                );
                scope.directory = after.directory.clone();
                if qualified {
                    scope.directory.failures = outer_failures.map(|mut outer| {
                        cwd::extend_unique(&mut outer, failures.clone());
                        outer
                    });
                    if scope.directory.failures.is_none() {
                        scope
                            .directory
                            .merge(failures.into_iter(), self.frontend.host.home);
                    }
                }
                self.merge_bindings(scope, &[before, left_exit, after]);
            } else {
                self.merge_bindings(scope, &[left_exit, after.clone()]);
                self.merge_directories(scope, &[after]);
            }
        }
        Ok(())
    }
    fn loop_body_is_invariant(
        &self,
        body: &[Statement],
        scope: &Scope,
        variable: &str,
    ) -> Result<bool, CheckError> {
        let mut inputs = BTreeSet::new();
        let mut writes = BTreeSet::new();
        let mut stable = BTreeSet::new();
        let local = BTreeSet::from([variable.to_owned()]);
        // A body without a carried input needs one candidate-union analysis.
        // Stateful builtins, functions, repeated loop names and dynamic writes
        // retain the sequential/convergence owner and its conservative limits.
        Ok(
            self.loop_inputs(body, scope, &local, &mut inputs, &mut writes, &mut stable)?
                && inputs
                    .iter()
                    .all(|name| !writes.contains(name) || stable.contains(name)),
        )
    }
    fn loop_word_inputs(
        &self,
        word: &RawWord,
        scope: &Scope,
        local: &BTreeSet<String>,
        inputs: &mut BTreeSet<String>,
        writes: &mut BTreeSet<String>,
        stable: &mut BTreeSet<String>,
    ) -> Result<Option<super::Expanded>, CheckError> {
        let variables = scope.contexts();
        let cwd = scope.directory.current.render();
        let expanded = super::words::expand(
            &word.raw,
            &word.syntax,
            &super::words::ExpansionContext {
                variables: &variables,
                runtime_variables: &BTreeSet::new(),
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
                .any(|e| matches!(e, super::RawExpansion::Variable(_)))
        {
            return Ok(None);
        }
        for expression in &expanded.arithmetic {
            let evaluation = super::arithmetic::evaluate(expression, &scope.values())?;
            if evaluation.bounded || !evaluation.code.is_empty() {
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
            if !local.contains(name) {
                inputs.insert(name.clone());
            }
        }
        if identifier(&expanded.word.text) && !local.contains(&expanded.word.text) {
            inputs.insert(expanded.word.text.clone());
        }
        let mut sources = expanded.nested.clone();
        for expansion in &word.expansions {
            if let super::RawExpansion::Code(source) = expansion
                && !sources.contains(source)
            {
                sources.push(source.clone());
            }
        }
        for source in sources {
            let Some(body) = super::brush::records(&source, &source)?.records else {
                return Ok(None);
            };
            if !self.loop_inputs(&body, scope, local, inputs, writes, stable)? {
                return Ok(None);
            }
        }
        Ok(Some(expanded))
    }
    fn loop_inputs(
        &self,
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
                    if !assignments.is_empty() && !argv.is_empty() {
                        return Ok(false);
                    }
                    for (name, word) in assignments {
                        if !identifier(name) || local.contains(name) {
                            return Ok(false);
                        }
                        let Some(value) =
                            self.loop_word_inputs(word, scope, local, inputs, writes, stable)?
                        else {
                            return Ok(false);
                        };
                        if !value.word.vars.is_empty() {
                            return Ok(false);
                        }
                        if value.word.expands && !value.nested.is_empty() {
                            stable.insert(name.clone());
                        } else if value.arithmetic.len() == 1 {
                            let evaluation =
                                super::arithmetic::evaluate(&value.arithmetic[0], &scope.values())?;
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
                    let mut words = Vec::new();
                    for word in argv {
                        let Some(value) =
                            self.loop_word_inputs(word, scope, local, inputs, writes, stable)?
                        else {
                            return Ok(false);
                        };
                        words.push(value.word);
                    }
                    let resolved = super::argv::resolve(
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
                            || self.functions.contains_key(&program.text)
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
                            || program == "printf"
                                && words.get(index + 1).is_some_and(|w| w == "-v")
                        {
                            return Ok(false);
                        }
                    }
                    for redirect in redirects {
                        if self
                            .loop_word_inputs(
                                &redirect.target,
                                scope,
                                local,
                                inputs,
                                writes,
                                stable,
                            )?
                            .is_none()
                        {
                            return Ok(false);
                        }
                    }
                }
                _ => return Ok(false),
            }
        }
        Ok(true)
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
    fn read(
        &mut self,
        args: &[crate::record::Word],
        targets: &[crate::record::Redirect],
        scope: &mut Scope,
    ) {
        let mut names = Vec::new();
        let mut index = 0;
        let mut raw = false;
        let mut delimiter = '\n';
        let mut count = None;
        let mut input_fd = 0;
        let mut modeled = true;
        let mut rejected = false;
        while let Some(word) = args.get(index) {
            if word == "--" {
                index += 1;
                break;
            }
            let Some(flags) = word.strip_prefix('-').filter(|flags| !flags.is_empty()) else {
                break;
            };
            for (at, option) in flags.char_indices() {
                match option {
                    'r' => raw = true,
                    's' => {}
                    'd' | 'n' | 't' | 'u' | 'a' | 'p' => {
                        let attached = &flags[at + option.len_utf8()..];
                        let value = if attached.is_empty() {
                            index += 1;
                            args.get(index).map(|word| {
                                modeled &= !word.expands;
                                word.text.as_str()
                            })
                        } else {
                            modeled &= !word.expands;
                            Some(attached)
                        };
                        let Some(value) = value else {
                            modeled = false;
                            break;
                        };
                        match option {
                            'd' => delimiter = value.chars().next().unwrap_or('\0'),
                            'n' => match value.parse::<usize>() {
                                Ok(value) => count = Some(value),
                                Err(_) => modeled = false,
                            },
                            't' => modeled &= value.parse::<f64>().is_ok_and(|value| value > 0.0),
                            'u' => match value.parse::<usize>() {
                                Ok(value) => input_fd = value,
                                Err(_) => modeled = false,
                            },
                            'a' => {
                                rejected = true;
                                if identifier(value) {
                                    names.push(value.to_owned());
                                }
                            }
                            'p' => rejected = true,
                            _ => unreachable!(),
                        }
                        break;
                    }
                    _ => modeled = false,
                }
            }
            index += 1;
        }
        for word in &args[index.min(args.len())..] {
            if identifier(&word.text) {
                names.push(word.text.clone());
            } else {
                modeled = false;
            }
        }
        let literal = targets.iter().rev().find(|target| {
            matches!(
                target.direction,
                crate::record::Direction::Heredoc | crate::record::Direction::Herestring
            ) && !target.expands
        });
        let eof = targets.iter().any(|target| {
            target.direction == crate::record::Direction::In
                && target.target == "/dev/null"
                && !target.expands
        });
        let process = targets.iter().rev().find(|target| {
            target.direction == crate::record::Direction::In && target.stream.is_some()
        });
        let input = if let Some(process) = process {
            match process.stream.as_deref() {
                Some(crate::record::StreamOutput::Known(output)) => Some(output.clone()),
                _ => None,
            }
        } else if let Some(literal) = literal {
            Some(vec![literal.target.clone()])
        } else if eof {
            Some(vec![String::new()])
        } else if targets.iter().any(|target| {
            matches!(
                target.direction,
                crate::record::Direction::In
                    | crate::record::Direction::Heredoc
                    | crate::record::Direction::Herestring
            )
        }) {
            None
        } else {
            scope.pipeline_input.clone()
        };
        modeled &= !names.is_empty()
            && !scope.bindings.contains_key("IFS")
            && (raw
                || input
                    .as_ref()
                    .is_none_or(|values| values.iter().all(|value| !value.contains('\\'))));
        let known_input = modeled && !rejected && input_fd == 0;
        let fields = if known_input {
            input.as_ref().map(|values| {
                values
                    .iter()
                    .map(|value| {
                        let data = value
                            .split(delimiter)
                            .next()
                            .unwrap_or("")
                            .chars()
                            .take(count.unwrap_or(usize::MAX))
                            .collect::<String>();
                        let mut remainder = data.as_str();
                        (0..names.len())
                            .map(|position| {
                                remainder = remainder.trim_start_matches([' ', '\t', '\n']);
                                let (field, rest) = if position + 1 == names.len() {
                                    (remainder.trim_end_matches([' ', '\t', '\n']), "")
                                } else {
                                    let end = remainder
                                        .find([' ', '\t', '\n'])
                                        .unwrap_or(remainder.len());
                                    (&remainder[..end], &remainder[end..])
                                };
                                remainder = rest;
                                field.to_owned()
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            })
        } else {
            None
        };
        for (position, name) in names.into_iter().enumerate() {
            let mut values = Vec::new();
            if let Some(fields) = &fields {
                for candidate in fields {
                    let value = BindingValue::Known(candidate[position].clone());
                    if !values.contains(&value) {
                        values.push(value);
                    }
                }
            } else {
                values.push(BindingValue::RuntimeUnknown(None));
            }
            // zsh rejects -a/-p, and -n writes an empty value rather than bash's prefix.
            if count.is_some() && !values.contains(&BindingValue::Known(String::new())) {
                values.push(BindingValue::Known(String::new()));
            }
            if (rejected || !modeled || input_fd != 0)
                && let Some(prior) = scope.bindings.get(&name)
            {
                for value in &prior.values {
                    if !values.contains(value) {
                        values.push(value.clone());
                    }
                }
            }
            scope.assign(name, values);
        }
        if !known_input
            && input.as_ref().is_some_and(|values| {
                values.iter().any(|value| {
                    !matches!(
                        super::arithmetic::armed(&value.replace('\\', "")),
                        super::arithmetic::Arming::Inert
                    )
                })
            })
        {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
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
                scope.local(&word.text);
                scope.assign(word.text.clone(), vec![BindingValue::Known(String::new())]);
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
        scope.local("#");
        scope.assign(
            "#".into(),
            vec![BindingValue::Known(argv.len().to_string())],
        );
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
            let binding = scope.expanded_binding(word, &word.text);
            scope.local(&name);
            scope.assign(
                name,
                vec![if matches!(binding, BindingValue::RepeatedFields(_)) {
                    binding
                } else if word.shell_matches && word.expands {
                    BindingValue::ShellDerived(word.text.clone())
                } else if word.shell_matches {
                    BindingValue::ShellMatches(word.text.clone())
                } else if word.expands {
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
                        let texts = match value {
                            BindingValue::RepeatedFields(repetition) => repetition.projections(),
                            _ => value.lexical().into_iter().cloned().collect(),
                        };
                        for text in texts {
                            let mut word = crate::record::Word::literal(text.clone());
                            // Assignment expansion has already consumed any unquoted tilde.
                            word.raw = format!("'{text}'");
                            word.expands = matches!(
                                value,
                                BindingValue::RuntimeDerived(_) | BindingValue::ShellDerived(_)
                            );
                            word.shell_matches = matches!(
                                value,
                                BindingValue::ShellMatches(_) | BindingValue::ShellDerived(_)
                            );
                            word.cardinality_unknown =
                                matches!(value, BindingValue::RepeatedFields(_));
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
                        BindingValue::RepeatedFields(_)
                            | BindingValue::RuntimeUnknown(_)
                            | BindingValue::RuntimeDerived(_)
                            | BindingValue::ShellDerived(_)
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
        let effects = crate::targets::infer(&command, &command.cwd, self.frontend.host);
        let cwd_dependent = command.argv.iter().any(|word| word.pwd)
            || !effects.code.is_empty()
            || !effects.inline.is_empty()
            || effects
                .targets
                .iter()
                .any(|target| target.effect != crate::record::Effect::Name || target.glob);
        for cwd in scope
            .directory
            .alternatives
            .iter()
            .filter(|_| cwd_dependent)
        {
            let cwd = cwd.render();
            let mut copy = command.clone();
            copy.cwd = cwd.clone();
            for word in &mut copy.argv {
                if word.pwd {
                    word.reproject_cwd(&cwd);
                }
            }
            if !command.nested
                && command.pipeline.is_none()
                && !scope.piped
                && self.output.script.commands.contains(&copy)
            {
                continue;
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
        if scope.directory.gap == Some(CoverageGap::IdentityBound)
            && operand.is_some_and(|index| !args[index].starts_with('/'))
        {
            // Relative movement cannot refine an unresolved cwd domain.
            return;
        }
        if operand.is_some_and(|index| {
            (args[index].globs || args[index].shell_matches) && !args[index].starts_with('/')
        }) {
            scope.relative_glob_moves += 1;
        }
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
        let physical = modes.contains(&b'P')
            && operand.is_none_or(|i| !args[i].expands && !args[i].globs && !args[i].shell_matches);
        let disputed = physical && modes.last() == Some(&b'L');
        let mut targets = vec![target.clone()];
        let oldpwd = program == "cd" && target == "-";
        if oldpwd {
            targets.clear();
            if let Some(binding) = scope.bindings.get("OLDPWD") {
                for value in &binding.values {
                    match value {
                        BindingValue::Known(value) => targets.push(value.clone()),
                        BindingValue::RepeatedFields(_) | BindingValue::Undetermined => {
                            self.output.gap(CoverageGap::UnsupportedShellSyntax);
                        }
                        BindingValue::RuntimeUnknown(_)
                        | BindingValue::RuntimeDerived(_)
                        | BindingValue::ShellMatches(_)
                        | BindingValue::ShellDerived(_) => {}
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
                        BindingValue::RepeatedFields(_) | BindingValue::Undetermined => {
                            self.output.gap(CoverageGap::UnsupportedShellSyntax);
                        }
                        BindingValue::RuntimeUnknown(_)
                        | BindingValue::RuntimeDerived(_)
                        | BindingValue::ShellMatches(_)
                        | BindingValue::ShellDerived(_) => {}
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
                } else if matches!(
                    value,
                    BindingValue::RepeatedFields(_) | BindingValue::Undetermined
                ) {
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
        if program == "cd"
            && targets.iter().any(|target| {
                !target.starts_with('/')
                    && target
                        .split('/')
                        .any(|part| !["", ".", ".."].contains(&part))
            })
        {
            scope.directory.relative_growth += 1;
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
                    || matches!(value, BindingValue::RuntimeDerived(text) | BindingValue::ShellDerived(text)
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

fn compatible_arguments(previous: &[crate::record::Word], choice: &[crate::record::Word]) -> bool {
    previous
        .iter()
        .filter(|word| !matches!(word.role, Role::Assign | Role::Precommand))
        .all(|left| {
            choice.iter().all(|right| {
                left.binding_candidates.iter().all(|(name, value)| {
                    right
                        .binding_candidates
                        .get(name)
                        .is_none_or(|other| other == value)
                })
            })
        })
}

#[cfg(test)]
mod candidate_cost {
    #[test]
    fn substitution_body_is_observed_once_per_candidate_union() {
        for width in [2, 4, 8] {
            let mut output = super::Observation::default();
            let host = crate::record::HostFacts {
                home: "/synthetic/home",
                user: None,
            };
            let mut evaluator = super::Evaluator::new(
                super::Frontend {
                    arm: crate::shell::Arm::Brush,
                    zsh: true,
                    host,
                },
                &mut output,
            );
            let mut scope = super::Scope::new(host.home, "/synthetic/project");
            for name in ["a", "b"] {
                scope.assign(
                    name.into(),
                    (0..width)
                        .map(|n| super::BindingValue::Known(format!("public{n}")))
                        .collect(),
                );
            }
            evaluator
                .expand(
                    &super::RawWord {
                        raw: "${a}$(printf public)${b}".into(),
                        syntax: super::WordSyntax::Shell,
                        expansions: Vec::new(),
                    },
                    &mut scope,
                    0,
                )
                .unwrap();
            assert_eq!(output.source_entries, 1, "width={width}");
            assert_eq!(output.parse_successes, 1, "width={width}");
            assert!(output.gaps.is_empty(), "{:?}", output.gaps);
        }
    }
    #[test]
    fn nested_substitution_work_grows_polynomially() {
        let count = |levels| {
            let names = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'];
            let mut source = String::from("P=1; F=2; ");
            for name in names.iter().take(levels) {
                source.push_str(&format!("for {name} in \"\" \"$P\" \"$F\"; do "));
            }
            source.push_str("o=$(sh x.sh");
            for name in names.iter().take(levels) {
                source.push_str(&format!(" \"${name}\""));
            }
            source.push_str(");");
            source.push_str(&" done;".repeat(levels));
            let output = crate::shell::observe(
                &source,
                crate::shell::Arm::Brush,
                "/synthetic/home",
                "/synthetic/work",
                true,
            )
            .unwrap();
            assert!(
                output
                    .script
                    .commands
                    .iter()
                    .any(|c| c.argv.iter().any(|w| w.text == "x.sh")),
                "nested script operand was lost"
            );
            assert!(output.source_entries > 0);
            assert!(
                output.source_entries <= levels + 1,
                "nested source body reparsed for loop combinations: levels={levels}, entries={}",
                output.source_entries
            );
            (
                output.source_entries,
                output.parse_successes + output.parse_failures,
                output.candidate_pairs,
                output.script.commands.len(),
            )
        };
        let counts = [2, 4, 8].map(count);
        println!("nested substitution work={counts:?}");
        for (small, large) in counts.iter().zip(counts.iter().skip(1)) {
            assert!(
                large.0 <= small.0 * 4
                    && large.1 <= small.1 * 4
                    && large.2 <= small.2 * 8
                    && large.3 <= small.3 * 4,
                "nested substitution work: {counts:?}"
            );
        }
    }

    #[test]
    fn repeated_binding_work_grows_quadratically() {
        let observe = |size| {
            let items = (0..size)
                .map(|n| format!("v{n}"))
                .collect::<Vec<_>>()
                .join(" ");
            let source = format!(
                "for id in {items}; do curl https://example.test/$id -o file_$id -w $id; done"
            );
            let output =
                crate::shell::observe(&source, crate::shell::Arm::Brush, "/h", "/h/p", true)
                    .unwrap();
            assert!(output.gaps.is_empty(), "{:?}", output.gaps);
            output.candidate_pairs
        };
        let small = observe(8);
        let large = observe(16);
        println!("candidate pairs: {small} -> {large}");
        assert!(
            small > 0 && large <= small * 5,
            "candidate pair growth: {small} -> {large}"
        );
    }
    #[test]
    fn glob_directory_work_stays_bounded() {
        let count = |size, isolated, branch| {
            let mut source = String::new();
            for n in 0..size {
                let body = match branch {
                    0 => format!("cd \"$d{n}\"; ls"),
                    1 => format!("if printf public; then cd \"$d{n}\"; fi; ls"),
                    _ => format!("printf public && cd \"$d{n}\"; ls"),
                };
                let body = if isolated { format!("({body})") } else { body };
                source.push_str(&format!("for d{n} in public*/; do {body}; done;"));
            }
            let output =
                crate::shell::observe(&source, crate::shell::Arm::Brush, "/h", "/h/p", true)
                    .unwrap();
            if isolated {
                assert!(output.gaps.is_empty(), "{:?}", output.gaps);
            } else {
                assert!(
                    output.gaps.contains(&crate::CoverageGap::IdentityBound),
                    "{:?}",
                    output.gaps
                );
                assert!(
                    !output.gaps.contains(&crate::CoverageGap::InspectionBudget),
                    "{:?}",
                    output.gaps
                );
            }
            output.candidate_pairs
        };
        for (isolated, branch) in [(false, 0), (true, 0), (false, 1), (false, 2)] {
            let small = count(2, isolated, branch);
            let large = count(4, isolated, branch);
            println!(
                "isolated={isolated}, branch={branch}: glob directory candidate pairs: {small} -> {large}"
            );
            assert!(
                small > 0 && large <= small * 4,
                "glob directory work: {small} -> {large}"
            );
        }
    }
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

    fn expand_candidates(
        values: Vec<BindingValue>,
        expansions: Vec<super::super::RawExpansion>,
        depth: usize,
    ) -> (Result<Vec<Expanded>, CheckError>, Observation) {
        let mut scope = Scope::new("/h", "/p");
        scope.assign("v".into(), values);
        let raw = RawWord {
            raw: "$v".into(),
            syntax: WordSyntax::Shell,
            expansions,
        };
        let mut output = Observation::default();
        let result = {
            let mut evaluator = Evaluator::new(
                Frontend {
                    arm: Arm::Brush,
                    zsh: true,
                    host: HostFacts {
                        home: "/h",
                        user: None,
                    },
                },
                &mut output,
            );
            super::super::expand_scoped(&raw, &mut scope, &mut evaluator, depth, false)
        };
        (result, output)
    }

    #[test]
    fn empty_runtime_candidate_survives_without_erasing_known_alternatives() {
        let (sole, _) = expand_candidates(
            vec![BindingValue::RuntimeUnknown(Some(String::new()))],
            Vec::new(),
            0,
        );
        let sole = sole.unwrap();
        assert_eq!(sole.len(), 1);
        assert!(sole[0].word.runtime_unknown);
        assert!(sole[0].word.text.is_empty());
        let (mixed, _) = expand_candidates(
            vec![
                BindingValue::RuntimeUnknown(Some(String::new())),
                BindingValue::Known("public".into()),
            ],
            Vec::new(),
            0,
        );
        assert!(
            mixed
                .unwrap()
                .iter()
                .any(|expanded| expanded.word.text == "public")
        );
    }

    #[test]
    fn runtime_absence_and_undetermined_binding_have_distinct_coverage() {
        for (value, unsupported) in [
            (BindingValue::RuntimeUnknown(None), false),
            (BindingValue::Undetermined, true),
        ] {
            let (expanded, output) = expand_candidates(vec![value], Vec::new(), 0);
            assert!(!expanded.unwrap().is_empty());
            assert_eq!(
                output.gaps.contains(&CoverageGap::UnsupportedShellSyntax),
                unsupported
            );
        }
    }

    #[test]
    fn independently_detected_code_obeys_the_source_recursion_frontier() {
        let mut output = Observation::default();
        let mut scope = Scope::new("/h", "/p");
        let mut evaluator = Evaluator::new(
            Frontend {
                arm: Arm::Brush,
                zsh: true,
                host: HostFacts {
                    home: "/h",
                    user: None,
                },
            },
            &mut output,
        );
        assert_eq!(
            evaluator
                .source("if then; cat =(true)", &mut scope, 64)
                .unwrap_err()
                .kind,
            crate::CheckErrorKind::ResourceLimit
        );
    }

    #[test]
    fn forwarded_code_and_variable_bodies_obey_the_recursive_frontier() {
        for expansion in [
            super::super::RawExpansion::Code("true".into()),
            super::super::RawExpansion::Variable("v".into()),
        ] {
            let (result, _) = expand_candidates(
                vec![BindingValue::Known("true".into())],
                vec![expansion],
                64,
            );
            assert!(
                matches!(
                    result,
                    Err(CheckError {
                        kind: crate::CheckErrorKind::ResourceLimit
                    })
                ),
                "forwarded body crossed its recursion frontier"
            );
        }
        for source in ["cat =(cat .env)", "v='cat .env'; echo ${(e)v}"] {
            let output = observation(source, &mut Scope::new("/h", "/p"));
            let protected: Vec<_> = output
                .script
                .commands
                .iter()
                .filter(|c| {
                    c.argv.iter().map(|w| w.text.as_str()).collect::<Vec<_>>() == ["cat", ".env"]
                })
                .collect();
            assert!(!protected.is_empty(), "forwarded child body was lost");
            assert!(
                protected.iter().all(|command| command.nested),
                "forwarded body acquired a top-level copy"
            );
        }
    }

    #[test]
    fn assigned_pwd_does_not_duplicate_non_tilde_words() {
        let mut scope = Scope::new("/h", "/p");
        scope.assign("PWD".into(), vec![BindingValue::Known("/assigned".into())]);
        let raw = RawWord {
            raw: "public".into(),
            syntax: WordSyntax::Shell,
            expansions: Vec::new(),
        };
        let mut output = Observation::default();
        let mut evaluator = Evaluator::new(
            Frontend {
                arm: Arm::Brush,
                zsh: true,
                host: HostFacts {
                    home: "/h",
                    user: None,
                },
            },
            &mut output,
        );
        assert_eq!(
            super::super::expand_scoped(&raw, &mut scope, &mut evaluator, 0, false)
                .unwrap()
                .len(),
            1
        );
        let output = observation(
            "PWD=/assigned; printf '%s' a b c d e f g h i j; printf '%s' ~+",
            &mut Scope::new("/h", "/p"),
        );
        assert!(!output.gaps.contains(&CoverageGap::InspectionBudget));
        assert!(
            output
                .script
                .commands
                .iter()
                .any(|c| c.argv.iter().any(|w| w.text == "/assigned"))
        );
        assert!(
            output
                .script
                .commands
                .iter()
                .any(|c| c.argv.iter().any(|w| w.text == "/p"))
        );
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
            scope.bindings["D"].values.iter().any(|value| value
                .lexical()
                .is_some_and(|text| text == "public*")
                && value.known().is_none()),
            "{:?}: {result:?}",
            scope.bindings
        );
        assert!(
            result
                .script
                .commands
                .iter()
                .flat_map(|command| &command.argv)
                .any(|word| word.text == "public*" && word.shell_matches)
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
    fn relative_cd_cannot_refine_a_carried_glob_cwd() {
        let mut scope = Scope::new("/h", "/h/project");
        let result = observation("for d in public*/; do cd \"$d\"; done; cd ..", &mut scope);
        assert!(result.gaps.contains(&CoverageGap::IdentityBound));
        assert_eq!(scope.directory.current.render(), "/h/project");
        assert!(scope.directory.alternatives.is_empty());
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
            cwd::bounded(&current, paths[..16].to_vec(), "/h"),
            (paths[..16].to_vec(), None)
        );
        assert_eq!(
            cwd::bounded(&current, paths, "/h"),
            (
                vec![
                    cwd::CwdPath::Logical("/h".into()),
                    cwd::CwdPath::Logical("/h/Library".into())
                ],
                Some(crate::CoverageGap::InspectionBudget)
            )
        );
    }
}
