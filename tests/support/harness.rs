#![allow(dead_code)]

use agent_guard_rust::{
    CheckError, CheckErrorKind, Context, Coverage, CoverageGap, Disposition, Evaluation, Event,
    Outcome,
    adapters::{Consumer, render},
    evaluate_with_arm,
    filesystem::{self, Probe},
    shell::Arm,
};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub fn rows() -> Vec<Value> {
    include_str!("../fixtures/rust-slice-dev.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

pub fn text<'a>(value: &'a Value, field: &str) -> &'a str {
    value[field].as_str().unwrap()
}

pub struct Fixture {
    pub root: PathBuf,
    pub home: String,
    pub project: String,
    pub container: String,
}

impl Fixture {
    pub fn new_at(scratch: &Path) -> Self {
        let root = scratch.join("fixtures").join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let home = root.join("home").to_str().unwrap().to_owned();
        let project = format!("{home}/project");
        let container = format!("{home}/Library/Containers/com.example.fixture");
        for dir in [&project, &container, &format!("{home}/.ssh")] {
            fs::create_dir_all(dir).unwrap();
        }
        for (path, bytes) in [
            (
                format!("{project}/input.txt"),
                "needle\n.env\nprocess.env.SHELL\n",
            ),
            (format!("{project}/notes.txt"), "ORIGINAL_NOTES\n"),
            (format!("{project}/untracked-work.txt"), "UNTRACKED_WORK\n"),
            (format!("{project}/out.txt"), "KEEP\n"),
            (format!("{project}/.env"), "SYNTHETIC_CANARY\n"),
            (format!("{home}/.ssh/id_rsa"), "SYNTHETIC_CANARY\n"),
            (format!("{container}/data.txt"), "SYNTHETIC_CANARY\n"),
        ] {
            fs::write(path, bytes).unwrap();
        }
        std::os::unix::fs::symlink(
            format!("{home}/Library/Containers"),
            format!("{project}/data-link"),
        )
        .unwrap();
        Self {
            root,
            home,
            project,
            container,
        }
    }
    pub fn expand(&self, value: &str) -> String {
        let mut result = value.to_owned();
        for (marker, replacement) in [
            ("$C", self.container.as_str()),
            ("$P", self.project.as_str()),
            ("$H", self.home.as_str()),
            ("$E", self.root.to_str().unwrap()),
            ("$R", self.root.to_str().unwrap()),
            ("$U", "fixture-user"),
        ] {
            let mut cursor = 0;
            while let Some(relative) = result[cursor..].find(marker) {
                let start = cursor + relative;
                let end = start + marker.len();
                if result
                    .as_bytes()
                    .get(end)
                    .is_none_or(|c| !c.is_ascii_alphanumeric() && *c != b'_')
                {
                    result.replace_range(start..end, replacement);
                    cursor = start + replacement.len();
                } else {
                    cursor = end;
                }
            }
        }
        result
    }
    pub fn expand_value(&self, value: &Value) -> Value {
        match value {
            Value::String(s) => Value::String(self.expand(s)),
            Value::Array(a) => Value::Array(a.iter().map(|v| self.expand_value(v)).collect()),
            Value::Object(o) => Value::Object(
                o.iter()
                    .map(|(k, v)| (k.clone(), self.expand_value(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    pub fn setup(&self, row: &Value) {
        if let Some(files) = row["synthetic_setup"]["files"].as_object() {
            for (path, bytes) in files {
                let path = self.expand(path);
                fs::create_dir_all(Path::new(&path).parent().unwrap()).unwrap();
                fs::write(path, self.expand(bytes.as_str().unwrap())).unwrap();
            }
        }
        if let Some(links) = row["synthetic_setup"]["links"].as_array() {
            for pair in links {
                std::os::unix::fs::symlink(
                    self.expand(pair[1].as_str().unwrap()),
                    self.expand(pair[0].as_str().unwrap()),
                )
                .unwrap();
            }
        }
    }
    pub fn context(&self, row: &Value) -> Context {
        let consumer = match text(row, "consumer") {
            "claude" => Consumer::Claude,
            "codex" => Consumer::Codex,
            "pi" => Consumer::Pi,
            _ => panic!("not a consumer"),
        };
        Context {
            consumer,
            home: self.home.clone(),
            user: Some("fixture-user".into()),
            cwd: self.expand(text(row, "cwd")),
            zsh_executor: consumer != Consumer::Pi,
            require_execution_owner: row["provenance_form"] == "required_execution_domain",
            shell_observation_entries: Cell::new(0),
        }
    }
    pub fn body(&self, row: &Value) -> Vec<u8> {
        if let Some(raw) = row["input"]["raw_event_bytes"].as_str() {
            return raw.as_bytes().to_vec();
        }
        let mut input = self.expand_value(&row["input"]);
        if let Some(recipe) = row["input"].get("generator") {
            let excess = usize::from(recipe["point"] == "one-unit-excess");
            let bound = recipe["bound"].as_u64().unwrap() as usize;
            let base = self.expand(recipe["base_command"].as_str().unwrap());
            let command = if recipe["kind"] == "nesting" {
                format!(
                    "{}{}{}",
                    "printf '%s' $(".repeat(bound + excess),
                    base,
                    ")".repeat(bound + excess)
                )
            } else {
                base
            };
            input = json!({"command":command});
            if recipe["kind"] == "input-bytes" {
                let event = json!({"tool_name":row["tool"],"tool_input":input});
                let length = serde_json::to_vec(&event).unwrap().len();
                input["command"] = json!(format!(
                    "{}{}",
                    input["command"].as_str().unwrap(),
                    " ".repeat(bound + excess - length)
                ));
            }
        }
        serde_json::to_vec(&json!({"tool_name":row["tool"],"tool_input":input})).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

pub struct RecordingProbe {
    pub calls: Vec<String>,
    pub stat_calls: Vec<String>,
    pub links: BTreeMap<String, String>,
    pub fault: Option<String>,
    pub home: String,
    pub patterned: bool,
}

impl RecordingProbe {
    pub fn new(fixture: &Fixture, word: &agent_guard_rust::record::Word) -> Self {
        Self {
            calls: Vec::new(),
            stat_calls: Vec::new(),
            links: BTreeMap::new(),
            fault: None,
            home: fixture.home.clone(),
            patterned: word.globs,
        }
    }
    pub fn literal_for_quoted_paths(fixture: &Fixture) -> Self {
        Self::new(
            fixture,
            &agent_guard_rust::record::Word::literal(String::new()),
        )
    }
    fn protected(&self, spelling: &str) -> bool {
        if self.patterned {
            filesystem::lexical(spelling, &self.home).is_some()
        } else {
            filesystem::lexical_literal(spelling, &self.home).is_some()
        }
    }
}
impl Probe for RecordingProbe {
    fn stat(&mut self, path: &Path) -> io::Result<Option<filesystem::Metadata>> {
        let spelling = path.to_str().unwrap().to_owned();
        self.stat_calls.push(spelling.clone());
        assert!(
            !self.protected(&spelling),
            "protected spelling reached stat: {spelling}"
        );
        if self.fault.as_ref() == Some(&spelling) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        filesystem::DiskProbe.stat(path)
    }
    fn read_link(&mut self, path: &Path) -> io::Result<Option<PathBuf>> {
        let spelling = path.to_str().unwrap().to_owned();
        self.calls.push(spelling.clone());
        assert!(
            !self.protected(&spelling),
            "protected spelling reached probe: {spelling}"
        );
        if self.fault.as_ref() == Some(&spelling) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        if let Some(target) = self.links.get(&spelling) {
            return Ok(Some(PathBuf::from(target)));
        }
        filesystem::DiskProbe.read_link(path)
    }
}

pub fn class(result: &Result<Evaluation, CheckError>) -> &'static str {
    match result {
        Err(_) => "F",
        Ok(e) => match e.outcome {
            Outcome::NoObjection => "N",
            Outcome::SoftAdvice(_) => "A",
            Outcome::ProtectedDenial { .. } => "D",
            Outcome::CoverageInsufficient {
                disposition: Disposition::ContinueLimitedPreflight,
                ..
            } => "UC",
            Outcome::CoverageInsufficient {
                disposition: Disposition::RejectUnsupportedSyntax,
                ..
            } => "UR",
            Outcome::CoverageInsufficient {
                disposition: Disposition::RequireVerifiedExecutionOwner,
                ..
            } => "UO",
        },
    }
}

fn gap_name(gap: &CoverageGap) -> String {
    match gap {
        CoverageGap::UnknownProgram { program } => format!("UnknownProgram:{program}"),
        CoverageGap::OutsideObservedTool { tool } => format!("OutsideObservedTool:{tool}"),
        other => format!("{other:?}"),
    }
}

pub fn coverage(result: &Result<Evaluation, CheckError>) -> Value {
    match result {
        Err(error) => json!({"state":"NotCompleted","error_kind":format!("{:?}",error.kind)}),
        Ok(e) => match &e.coverage {
            Coverage::SupportedPreflight => json!({"state":"SupportedPreflight"}),
            Coverage::LimitedPreflight(gaps) => {
                json!({"state":"LimitedPreflight","gaps":gaps.iter().map(gap_name).collect::<Vec<_>>()})
            }
            Coverage::OutsideObservedToolCoverage { tool } => {
                json!({"state":"OutsideObservedToolCoverage","tool_class":tool})
            }
        },
    }
}

pub struct Gate {
    pub operation_start_count: usize,
}

impl Gate {
    pub fn run(
        &mut self,
        consumer: Consumer,
        wire: &agent_guard_rust::adapters::Wire,
        operation: impl FnOnce() -> String,
    ) -> Option<String> {
        if agent_guard_rust::adapters::permits_call(consumer, wire) {
            self.operation_start_count += 1;
            Some(operation())
        } else {
            None
        }
    }
}

pub fn run(row: &Value, arm: Arm) -> Value {
    let fixture = Fixture::new();
    fixture.setup(row);
    let context = fixture.context(row);
    let body = fixture.body(row);
    let mut probe = RecordingProbe::literal_for_quoted_paths(&fixture);
    if row["fault_injection"]["operation"].as_str().is_some() {
        probe.fault = Some(fixture.project.clone());
    }
    let start = Instant::now();
    let mut lifecycle = Value::Null;
    let result = match row["fault_injection"]["owner"].as_str() {
        Some("offline checker lifecycle" | "offline checker worker") => {
            let kind = if text(row, "id").contains("deadline") {
                CheckErrorKind::Deadline
            } else if text(row, "id").contains("cancelled") {
                CheckErrorKind::Cancelled
            } else {
                CheckErrorKind::GuardFault
            };
            lifecycle = worker_fault(&fixture, kind);
            Err(CheckError { kind })
        }
        _ => evaluate_with_arm(
            Event {
                bytes: &body,
                context: &context,
                probe: &mut probe,
            },
            arm,
        ),
    };
    let wire = render(context.consumer, &result);
    let elapsed_ns = start.elapsed().as_nanos();
    let mut gate = Gate {
        operation_start_count: 0,
    };
    let mut witness = Witness::default();
    let task_result = gate.run(context.consumer, &wire, || {
        closed_operation(&fixture, row, &body, &mut witness)
    });
    if let Some(task_result) = &task_result {
        assert_task_result(&fixture, row, task_result, &witness);
    }
    let recovery = match &result {
        Ok(Evaluation {
            outcome: Outcome::ProtectedDenial { recovery, .. },
            ..
        })
        | Ok(Evaluation {
            outcome:
                Outcome::CoverageInsufficient {
                    recovery: Some(recovery),
                    ..
                },
            ..
        }) => agent_guard_rust::adapters::recovery_value(recovery),
        _ => Value::Null,
    };
    let recovery_receipt = verify_recovery(&fixture, row, &recovery, arm);
    let observed_effects = result
        .as_ref()
        .map(|evaluation| agent_guard_rust::adapters::effects_value(&evaluation.effects))
        .unwrap_or_else(|_| json!([]));
    json!({"id":row["id"],"consumer":row["consumer"],"arm":format!("{arm:?}"),"class":class(&result),"coverage":coverage(&result),"library":format!("{result:?}"),"recovery":recovery,"recovery_receipt":recovery_receipt,"exit":wire.exit,"stdout":wire.stdout,"stderr":wire.stderr,"probe_calls":probe.calls,"shell_observation_entries":context.shell_observation_entries.get(),"operation_start_count":gate.operation_start_count,"task_result":task_result,"task_witness_scope":if matches!(&result,Ok(Evaluation {coverage:Coverage::OutsideObservedToolCoverage {..},..})) || text(row,"id").contains("dynamic-chooser") {"known-public control; original task incomplete"} else {"closed fixture operation surrogate"},"observed_effects":observed_effects,"effects":witness.value(&fixture),"elapsed_ns":elapsed_ns,"lifecycle":lifecycle,"evidence_owner":if is_lifecycle_row(row) {"harness-only"} else {"evaluator"}})
}

pub fn is_lifecycle_row(row: &Value) -> bool {
    matches!(
        row["fault_injection"]["owner"].as_str(),
        Some("offline checker lifecycle" | "offline checker worker")
    )
}

pub fn is_evaluator_row(row: &Value) -> bool {
    row["consumer"] != "owned-writer" && !is_lifecycle_row(row)
}

#[derive(Default)]
struct Witness {
    reads: Vec<String>,
    writes: Vec<String>,
    names: Vec<String>,
}

impl Witness {
    fn read(&mut self, path: &str) -> String {
        self.reads.push(path.to_owned());
        fs::read_to_string(path).unwrap()
    }
    fn write(&mut self, path: &str, bytes: &str) -> String {
        self.writes.push(path.to_owned());
        fs::write(path, bytes).unwrap();
        fs::read_to_string(path).unwrap()
    }
    fn value(&self, fixture: &Fixture) -> Value {
        json!({"reads":self.reads,"writes":self.writes,"names":self.names,"protected_access_count":self.reads.iter().chain(&self.writes).filter(|p|filesystem::lexical(p,&fixture.home).is_some()).count()})
    }
}

// These are closed fixture operations. Submitted shell/interpreter code is never executed.
fn closed_operation(fixture: &Fixture, row: &Value, body: &[u8], witness: &mut Witness) -> String {
    let input: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let tool = input["tool_name"].as_str().unwrap_or("");
    let data = &input["tool_input"];
    let objective = text(row, "task_objective");
    if text(row, "id").starts_with("S15") || row["input"]["generator"]["kind"] == "nesting" {
        return witness.read(&format!("{}/input.txt", fixture.project));
    }
    if matches!(tool, "Write" | "write" | "Edit" | "edit") {
        let path = data
            .get("file_path")
            .or_else(|| data.get("path"))
            .and_then(Value::as_str)
            .unwrap();
        let bytes = if let Some(content) = data.get("content").and_then(Value::as_str) {
            content.to_owned()
        } else {
            let current = witness.read(path);
            let old = data
                .get("old_string")
                .or_else(|| data.get("oldText"))
                .and_then(Value::as_str)
                .unwrap();
            let new = data
                .get("new_string")
                .or_else(|| data.get("newText"))
                .and_then(Value::as_str)
                .unwrap();
            assert!(current.contains(old));
            current.replacen(old, new, 1)
        };
        return witness.write(path, &bytes);
    }
    if matches!(tool, "Read" | "read") {
        return witness.read(
            data.get("file_path")
                .or_else(|| data.get("path"))
                .and_then(Value::as_str)
                .unwrap(),
        );
    }
    let command = data["command"].as_str().unwrap_or("");
    if command.contains("--files") {
        witness.names = fs::read_dir(&fixture.project)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_str().unwrap().to_owned())
            .collect();
        witness.names.sort();
        assert!(witness.names.iter().any(|name| name == ".env"));
        return witness.names.join("\n");
    }
    if objective.contains("config") && Path::new(&format!("{}/conf.json", fixture.project)).exists()
    {
        let config = witness.read(&format!("{}/conf.json", fixture.project));
        let config: Value = serde_json::from_str(&config).unwrap();
        return witness.read(config["p"].as_str().unwrap());
    }
    if text(row, "id").contains("python-regex") {
        return if witness
            .read(&format!("{}/input.txt", fixture.project))
            .contains("process.env.SHELL")
        {
            "True\n"
        } else {
            "False\n"
        }
        .into();
    }
    if objective == "store fixture output in a file" {
        return witness.write(&format!("{}/out.txt", fixture.project), "fixture output\n");
    }
    if objective == "produce literal marker text" {
        return witness.write(&format!("{}/fixture.txt", fixture.project), ".env\n");
    }
    let observation = agent_guard_rust::shell::observe(
        command,
        Arm::Brush,
        &fixture.home,
        &fixture.project,
        true,
    )
    .unwrap();
    if command.contains("<<") {
        let body = command
            .split_once('\n')
            .map(|(_, body)| body.split("\nDOC").next().unwrap_or(body))
            .unwrap_or("");
        let body = format!("{body}\n");
        if let Some(redirect) = observation
            .script
            .commands
            .iter()
            .flat_map(|c| &c.redirects)
            .find(|r| r.direction == agent_guard_rust::record::Direction::Out)
        {
            return witness.write(&redirect.target, &body);
        }
        return body;
    }
    if objective == "emit a and b using compatible loop" {
        return ["a", "b"].map(|s| format!("{s}\n")).concat();
    }
    if let Some(args) = observation
        .script
        .commands
        .iter()
        .find(|c| c.argv.first().is_some_and(|s| s == "printf"))
        .map(|c| &c.argv)
    {
        if args.len() > 2 {
            return format!(
                "{}{}",
                args[2..]
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<String>(),
                if args[1].ends_with('\n') || args[1].ends_with("\\n") {
                    "\n"
                } else {
                    ""
                }
            );
        }
        if args.len() == 2 {
            return args[1].text.clone();
        }
    }
    if let Some(args) = observation
        .script
        .commands
        .iter()
        .find(|c| c.argv.first().is_some_and(|s| s == "git"))
        .map(|c| &c.argv)
        && let Some(message) = args
            .iter()
            .position(|s| s == "-m")
            .and_then(|i| args.get(i + 1))
    {
        return witness.write(
            fixture.root.join("commit-message").to_str().unwrap(),
            message,
        );
    }
    if observation
        .script
        .commands
        .iter()
        .any(|c| c.argv.first().is_some_and(|s| s == "ls"))
    {
        witness.names = fs::read_dir(&fixture.project)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_str().unwrap().to_owned())
            .filter(|n| !n.starts_with('.'))
            .collect();
        witness.names.sort();
        if command.contains("+(a|b).txt") {
            witness.names.retain(|n| n == "a.txt" || n == "b.txt");
        }
        if command.contains("*(.)") {
            witness
                .names
                .retain(|name| !matches!(name.as_str(), "data-link" | "input-link"));
        }
        if command.contains("!(x)") {
            witness.names.retain(|n| n != "x");
        }
        return witness.names.join("\n");
    }
    if let Some(path) = observation
        .script
        .commands
        .iter()
        .filter(|c| c.argv.first().is_some_and(|s| s == "cat"))
        .flat_map(|c| c.argv.iter().skip(1))
        .find(|p| p.starts_with('/') && Path::new(p.as_str()).is_file())
    {
        return witness.read(path);
    }
    if objective.contains("path string") || command.contains("-F") {
        let path = format!("{}/path-mention.txt", fixture.project);
        if Path::new(&path).exists() {
            let result = witness.read(&path);
            assert!(result.contains(&fixture.container));
            return result;
        }
    }
    let contents = witness.read(&format!("{}/input.txt", fixture.project));
    if matches!(tool, "Grep" | "grep") {
        let pattern = data["pattern"].as_str().unwrap_or("needle");
        return contents
            .lines()
            .filter(|line| line.contains(pattern))
            .collect::<Vec<_>>()
            .join("\n");
    }
    if observation
        .script
        .commands
        .iter()
        .any(|c| c.argv.first().is_some_and(|s| s == "rg"))
    {
        let pattern = if command.contains("process.env.SHELL") {
            "process.env.SHELL"
        } else if command.contains(".env") {
            ".env"
        } else {
            "needle"
        };
        return contents
            .lines()
            .filter(|line| line.contains(pattern))
            .collect::<Vec<_>>()
            .join("\n");
    }
    contents
}

fn verify_recovery(fixture: &Fixture, row: &Value, recovery: &Value, arm: Arm) -> Value {
    if recovery.is_null() {
        return Value::Null;
    }
    let chosen = row["recovery_objective"]
        .get("next_operation")
        .filter(|next| next.get("tool").is_some() && next.get("input").is_some());
    let Some(next) = chosen else {
        return json!({"state":"owner_action","original_task_complete":false});
    };
    let next = fixture.expand_value(next);
    let mut context = fixture.context(row);
    context.cwd = fixture.project.clone();
    context.require_execution_owner = false;
    let body = serde_json::to_vec(
        &json!({"tool_name":next["tool"],"tool_input":next["input"],"cwd":fixture.project}),
    )
    .unwrap();
    let mut probe = RecordingProbe::literal_for_quoted_paths(fixture);
    let result = evaluate_with_arm(
        Event {
            bytes: &body,
            context: &context,
            probe: &mut probe,
        },
        arm,
    );
    if arm == Arm::StructuredOnly && matches!(next["tool"].as_str(), Some("Bash" | "bash")) {
        return json!({"state":"unsupported_arm","class":class(&result)});
    }
    assert_eq!(
        class(&result),
        "N",
        "{} recovery does not recheck: {:?}",
        row["id"],
        result
    );
    let mut witness = Witness::default();
    let mut gate = Gate {
        operation_start_count: 0,
    };
    let wire = render(context.consumer, &result);
    let task_result = gate.run(context.consumer, &wire, || {
        closed_operation(fixture, row, &body, &mut witness)
    });
    assert_eq!(gate.operation_start_count, 1);
    assert_eq!(witness.value(fixture)["protected_access_count"], 0);
    json!({"state":"rechecked","agent_continuation_witness":true,"next_operation":next,"operation_start_count":gate.operation_start_count,"task_result":task_result,"effects":witness.value(fixture),"original_whole_home_complete":false})
}

fn assert_task_result(fixture: &Fixture, row: &Value, result: &str, witness: &Witness) {
    let id = text(row, "id");
    if id.starts_with("S01-read-public")
        || id.starts_with("S01-read-alias-public")
        || id.starts_with("S20")
        || id.contains("runtime-config-public")
        || id.starts_with("S16-unknown-public")
    {
        assert_eq!(result, "needle\n.env\nprocess.env.SHELL\n");
    }
    if id.starts_with("S03") || id.starts_with("S17") {
        assert_eq!(result, "needle");
    }
    if id.starts_with("S06") {
        assert!(result.contains(&format!("{}/data.txt", fixture.container)));
        assert_eq!(
            witness.reads,
            vec![format!("{}/path-mention.txt", fixture.project)]
        );
    }
    if id.starts_with("S08-printf") {
        assert_eq!(result, format!("cat {}/data.txt\n", fixture.container));
    }
    if id.starts_with("S08-commit") {
        assert_eq!(result, format!("document cat {}/.env", fixture.project));
    }
    if id.starts_with("S09") {
        assert_eq!(result, format!("cat {}/data.txt\n", fixture.container));
        assert_eq!(
            witness.writes,
            vec![format!("{}/fixture.txt", fixture.project)]
        );
    }
    if id.starts_with("S19-unicode-heredoc") {
        assert_eq!(result, format!("路径 cat {}/data.txt\n", fixture.container));
    }
    if id.starts_with("S13-valid-loop") {
        assert_eq!(result, "a\nb\n");
    }
    if id.starts_with("S14-quoted-flags") {
        assert_eq!(result, "${(f)v}\n");
    }
    if id.starts_with("S14-quoted-qualifier") {
        assert_eq!(
            result,
            format!(
                "{}/*(e:cat {}/data.txt:)\n",
                fixture.project, fixture.container
            )
        );
    }
    if id.starts_with("S14-quoted-heredoc") {
        assert_eq!(
            result,
            format!(
                "${{(e)v}} {}/*(e:'cat {}/data.txt':)\n",
                fixture.project, fixture.container
            )
        );
    }
    if id.starts_with("S14-pattern-extglob") {
        assert_eq!(result, "a.txt\nb.txt");
    }
    if id.starts_with("S14-pattern-filter") {
        assert!(
            !witness
                .names
                .iter()
                .any(|name| name.starts_with('.') || name == "data-link")
        );
        assert!(witness.names.iter().any(|name| name == "input.txt"));
    }
    if id.starts_with("S14-pattern-negated") {
        assert!(!witness.names.iter().any(|name| name == "x"));
        assert!(witness.names.iter().any(|name| name == "input.txt"));
    }
    if id.starts_with("S11-write-public") || id.starts_with("S11-edit-public") {
        assert_eq!(result, format!("KEEP\ncat {}/.env\n", fixture.project));
        assert_eq!(witness.writes, vec![format!("{}/out.txt", fixture.project)]);
    }
    if id.starts_with("S11-redirect-public") {
        assert_eq!(result, "fixture output\n");
        assert_eq!(witness.writes, vec![format!("{}/out.txt", fixture.project)]);
    }
    if id.starts_with("D11-python-regex") {
        assert_eq!(result, "True\n");
    }
    if id.starts_with("D11-rg-pattern") {
        assert_eq!(result, ".env\nprocess.env.SHELL");
    }
    if id.starts_with("D11-structured-pattern") {
        assert_eq!(result, "process.env.SHELL");
    }
    if id.contains("runtime-config-protected") || id.contains("inline-config") {
        assert_eq!(result, "SYNTHETIC_CANARY\n");
    }
    if id.starts_with("S15") {
        assert_eq!(result, "needle\n.env\nprocess.env.SHELL\n");
        assert_eq!(witness.value(fixture)["protected_access_count"], 0);
    }
}

fn worker_fault(fixture: &Fixture, kind: CheckErrorKind) -> Value {
    use std::{
        process::{Command, Stdio},
        time::Duration,
    };
    let binary = Fixture::worker_binary();
    let receipt = fixture.root.join("worker-receipt");
    let started = Instant::now();
    let mut child = Command::new(binary)
        .arg(if kind == CheckErrorKind::GuardFault {
            "fault"
        } else {
            "wait"
        })
        .arg(&receipt)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    while fs::read_to_string(&receipt).ok().as_deref() != Some(&format!("ready:{pid}\n"))
        && started.elapsed() < Duration::from_secs(1)
    {
        std::thread::sleep(Duration::from_millis(1));
    }
    let ready = fs::read_to_string(&receipt).ok().as_deref() == Some(&format!("ready:{pid}\n"));
    if !ready {
        child.kill().unwrap();
        child.wait().unwrap();
        panic!("worker failed to become ready within fixture deadline");
    }
    if kind != CheckErrorKind::GuardFault {
        if kind == CheckErrorKind::Deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        child.kill().unwrap();
    }
    let status = child.wait().unwrap();
    assert_eq!(child.try_wait().unwrap(), Some(status));
    assert!(!status.success());
    let receipt = fs::read_to_string(receipt).unwrap();
    assert_eq!(receipt, format!("ready:{pid}\n"));
    assert!(started.elapsed() < Duration::from_secs(3));
    json!({"pid":pid,"ready_receipt":true,"completion_observed_by_wait":true,"reaped":true,"successful":false,"wall_us":started.elapsed().as_micros()})
}

pub fn assert_preflight_tuple(row: &Value, actual: &Value) {
    let conditional = row.get("conditional_outcome").is_some()
        && actual["observed_effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|effect| {
                effect["protection"] == "AppData"
                    && effect["write"] == false
                    && effect["source"] == "Nested"
            });
    let expected = if conditional {
        "D".to_owned()
    } else {
        row["outcome_class"].as_str().unwrap().replace('-', "")
    };
    assert_eq!(
        actual["class"].as_str().unwrap(),
        expected,
        "{}: {}",
        row["id"],
        actual["library"]
    );
    assert_eq!(
        actual["coverage"]["state"], row["expected_coverage"]["state"],
        "{} coverage",
        row["id"]
    );
    if let Some(tool) = row["expected_coverage"].get("tool_class") {
        assert_eq!(
            &actual["coverage"]["tool_class"], tool,
            "{} tool class",
            row["id"]
        );
    }
    if let Some(expected) = row["expected_coverage"]["gaps"].as_array() {
        let mut expected = expected.clone();
        expected.sort_by_key(Value::to_string);
        let mut got = actual["coverage"]["gaps"].as_array().unwrap().clone();
        got.sort_by_key(Value::to_string);
        if conditional {
            for required in expected {
                assert!(got.contains(&required), "{} conditional gap", row["id"]);
            }
        } else {
            assert_eq!(got, expected, "{} gaps", row["id"]);
        }
    }
    let class = actual["class"].as_str().unwrap();
    let expected_exit = if ["F", "D", "UR", "UO"].contains(&class) {
        2
    } else {
        0
    };
    assert_eq!(actual["exit"], expected_exit);
    if class == "A" {
        assert!(actual["stderr"].as_str().unwrap().is_empty());
    } else {
        assert!(actual["stdout"].as_str().unwrap().is_empty());
    }
    if ["N", "UC"].contains(&class) {
        assert!(actual["stderr"].as_str().unwrap().is_empty());
    }
    assert_eq!(
        !actual["stdout"].as_str().unwrap().is_empty(),
        row["advice_expectation"]["expectation"] == "present"
    );
    assert_effect_or_failure(row, actual, conditional);
}

fn assert_effect_or_failure(row: &Value, actual: &Value, conditional: bool) {
    let contract = if conditional {
        &row["conditional_outcome"]["reason_contract"]
    } else {
        &row["reason_contract"]
    };
    let Some(required) = contract["effect_or_failure"].as_str() else {
        assert_ne!(
            contract["emission"], "required",
            "required reason needs a semantic partition"
        );
        return;
    };
    let stderr = actual["stderr"].as_str().unwrap();
    let effects = actual["observed_effects"].as_array().unwrap();
    let protected = |kind: &str, write: bool| {
        effects
            .iter()
            .any(|effect| effect["protection"] == kind && effect["write"] == write)
    };
    let gaps = actual["coverage"]["gaps"].as_array();
    let gap = |name: &str| gaps.is_some_and(|values| values.iter().any(|value| value == name));
    let matches = if actual["class"] == "F" {
        actual["coverage"]["error_kind"] == required
    } else if required.contains("App Data") {
        protected("AppData", false)
            && stderr.contains("app-data")
            && (!required.contains("nested") && !required.contains("substitution")
                || effects.iter().any(|effect| {
                    effect["protection"] == "AppData" && effect["source"] == "Nested"
                }))
    } else if required.contains("SSH") || required.contains("private-key") {
        protected("SshPrivate", required.contains("write"))
            && (stderr.contains("credential or environment file")
                || stderr.contains("private material under ~/.ssh")
                || stderr.contains("private ~/.ssh"))
    } else if required.contains("broad") {
        effects.iter().any(|effect| effect["kind"] == "BroadRoot")
            && stderr.contains("home directory")
            && stderr.contains("Scope the scan to a project path")
    } else if required.contains("inline interpreter") {
        protected("Environment", false)
            && effects
                .iter()
                .any(|effect| effect["source"] == "InlineCode")
            && stderr.contains("inline code")
    } else if required.contains("hidden") {
        effects
            .iter()
            .any(|effect| effect["kind"] == "HiddenContent")
            && stderr.contains("hidden files")
    } else if required.contains("environment-file") {
        protected("Environment", false) && stderr.contains("environment")
    } else if required.contains("Zsh") || required.contains("zsh") {
        (gap("ExecutorDivergence") || gap("UnsupportedDialectConstruct"))
            && stderr.contains("unsupported")
    } else if required.contains("parse failure") || required.contains("redirection lacks") {
        gap("UnsupportedShellSyntax") && stderr.contains("shell syntax")
    } else if required.contains("execution owner") {
        gap("ExecutionOwnerUnavailable") && stderr.contains("execution owner")
    } else {
        false
    };
    assert!(
        matches,
        "{} required effect/failure {required}: effects={} coverage={} stderr={stderr}",
        row["id"], actual["observed_effects"], actual["coverage"]
    );
}

pub fn assert_tuple(row: &Value, actual: &Value) {
    assert_preflight_tuple(row, actual);
    let class = actual["class"].as_str().unwrap();
    assert_eq!(
        actual["operation_start_count"],
        if ["N", "A", "UC"].contains(&class) {
            1
        } else {
            0
        }
    );
    if text(row, "id").starts_with("S20") && class == "F" {
        assert_eq!(actual["shell_observation_entries"], 0);
        assert_eq!(actual["probe_calls"].as_array().unwrap().len(), 0);
        assert_eq!(actual["coverage"]["error_kind"], "ResourceLimit");
    }
    assert_observers(row, actual);
}

fn assert_observers(row: &Value, actual: &Value) {
    let id = text(row, "id");
    let class = actual["class"].as_str().unwrap();
    let stderr = actual["stderr"].as_str().unwrap();
    if ["D", "UR", "UO", "F"].contains(&class) {
        assert_eq!(actual["effects"]["protected_access_count"], 0);
    }
    if class == "D" {
        assert!(!stderr.is_empty());
        if id.contains("appdata") || id.contains("alias") || id.contains("qualifier-protected") {
            assert!(stderr.contains("app-data"));
        }
        if id.contains("credential") {
            assert!(
                stderr.contains("credential or environment file")
                    || stderr.contains("private material under ~/.ssh")
            );
        }
        if id.contains("env") && !id.contains("dump") {
            assert!(stderr.contains("environment"));
        }
        assert!(!stderr.contains("recovery:"));
        assert!(!actual["recovery"].is_null());
    }
    if class == "F" {
        let expected = row["reason_contract"]["effect_or_failure"]
            .as_str()
            .unwrap();
        assert_eq!(actual["coverage"]["error_kind"], expected);
        assert!(stderr.contains("recheck") && !stderr.contains("SYNTHETIC_CANARY"));
    }
    if class == "UR" {
        let excluded = actual["recovery"]["excluded_scope"].to_string();
        for (gap, required_scope) in [
            ("IdentityBound", "unresolved resource identity"),
            ("InspectionBudget", "over-budget function expansion"),
            ("UnsupportedShellSyntax", "original unsupported shell"),
        ] {
            if actual["coverage"]["gaps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == gap)
            {
                assert!(
                    excluded.contains(required_scope),
                    "{id} recovery boundary {gap}"
                );
            }
        }
    }
    if id.contains("deadline") || id.contains("cancelled") {
        assert_eq!(actual["lifecycle"]["ready_receipt"], true);
        assert_eq!(actual["lifecycle"]["reaped"], true);
        assert_eq!(actual["lifecycle"]["completion_observed_by_wait"], true);
    }
    if id.contains("hidden-names") {
        assert!(
            actual["effects"]["names"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s == ".env")
        );
        assert_eq!(actual["effects"]["protected_access_count"], 0);
    }
    if id.contains("inline-config") || id.contains("runtime-config-protected") {
        assert_eq!(actual["effects"]["protected_access_count"], 1);
        assert_eq!(class, "UC");
    }
    if class == "A" && row["consumer"] == "claude" {
        let output: Value = serde_json::from_str(actual["stdout"].as_str().unwrap()).unwrap();
        let context = output["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert_eq!(context.matches("rg -r means --replace.").count(), 1);
        assert_eq!(
            context,
            "rg -r means --replace. Drop -r; use -n for line numbers, or spell --replace VALUE for an intentional replacement."
        );
    }
    if row
        .get("recovery_objective")
        .is_some_and(|value| !value.is_null())
        && class != "F"
    {
        let recovery = &actual["recovery"];
        assert_eq!(recovery["automatic_application_supported"], false);
        assert!(
            recovery.get("objective").is_none(),
            "guard cannot claim agent intent"
        );
        assert_eq!(recovery["next_step"]["kind"], "owner_action");
        assert!(!recovery["preserved_scope"].as_array().unwrap().is_empty());
        if row["recovery_objective"]
            .get("next_operation")
            .is_some_and(|next| next.get("tool").is_some() && next.get("input").is_some())
        {
            assert_eq!(
                actual["recovery_receipt"]["state"], "rechecked",
                "{id} agent continuation"
            );
            assert_eq!(
                actual["recovery_receipt"]["agent_continuation_witness"],
                true
            );
            assert_eq!(
                actual["recovery_receipt"]["effects"]["protected_access_count"],
                0
            );
        } else {
            assert_eq!(actual["recovery_receipt"]["original_task_complete"], false);
        }
        if id.contains("search-home") || id.contains("shell-home") || id.contains("search-library")
        {
            let excluded = recovery["excluded_scope"].to_string();
            assert!(
                excluded.contains("HOME")
                    && excluded.contains("outside")
                    && excluded.contains("Library")
                    && excluded.contains(".ssh")
                    && excluded.contains("environment-file"),
                "{id} broad scope exclusions"
            );
            assert_eq!(
                actual["recovery_receipt"]["original_whole_home_complete"],
                false
            );
        }
    }
}
