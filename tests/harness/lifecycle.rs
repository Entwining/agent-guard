use super::{Result, process::*};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::os::unix::fs::OpenOptionsExt;
use std::{
    env,
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

pub const FAULTS: &[&str] = &[
    "pipe",
    "late-start",
    "startup-stall",
    "large-reason",
    "large-stderr",
    "slow-reason",
    "no-reason",
    "checker-failure",
    "panic",
    "partial-panic",
    "hang",
    "runner-stall",
    "early-failure",
    "leftover",
    "dependency-failure",
];
pub const CONTROLS: &[(&str, &str)] = &[
    ("drain", "large-reason"),
    ("stderr-drain", "large-stderr"),
    ("failclosed", "checker-failure"),
    ("deadline", "runner-stall"),
    ("cleanup", "leftover"),
    ("dependency", "dependency-failure"),
];
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessRecord {
    pub role: String,
    pub pid: i32,
    pub pgid: i32,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LifecycleResult {
    #[serde(flatten)]
    pub process: ProcessResult,
    pub processes: Vec<ProcessRecord>,
    pub alive_at_return: Vec<ProcessRecord>,
    pub alive_after_poll: Vec<ProcessRecord>,
    pub producer_bytes: usize,
}
#[derive(Debug)]
pub struct LifecycleFailure {
    pub result: LifecycleResult,
    pub cause: super::Error,
}
impl std::fmt::Display for LifecycleFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.cause.fmt(f)
    }
}
impl std::error::Error for LifecycleFailure {}
pub struct FaultPatch {
    pub runner: String,
    pub entry: String,
    pub links: String,
    pub fixture: String,
}
fn replace(text: &str, old: &str, next: &str) -> Result<String> {
    if text.matches(old).count() != 1 {
        return Err(format!("fault injection marker count differs: {old:?}").into());
    }
    Ok(text.replacen(old, next, 1))
}
pub fn inject_fault(
    original: &str,
    entry: &str,
    links: &str,
    fault: &str,
    control: &str,
) -> Result<FaultPatch> {
    let mut entry = entry.to_owned();
    let mut links = links.to_owned();
    let mut runner = replace(
        original,
        "use crate::{",
        "mod lifecycle_fixture;\n\nuse crate::{",
    )?;
    runner = replace(
        &runner,
        "    let deadline = Instant::now() + CHECKER_TIMEOUT;",
        "    if let Some(status) = lifecycle_fixture::check(input, output, error) {\n        return status;\n    }\n    let deadline = Instant::now() + CHECKER_TIMEOUT;",
    )?;
    if fault == "dependency-failure" {
        links = replace(
            &links,
            "std::fs::read_to_string(\"/usr/share/firmlinks\")",
            if control == "dependency" {
                "Ok(String::new())"
            } else {
                "Err(std::io::Error::other(\"synthetic initialization failure\"))"
            },
        )?;
    }
    runner = replace(
        &runner,
        "pub fn run(args: &[String]) -> io::Result<i32> {",
        "pub fn run(args: &[String]) -> io::Result<i32> {\n    lifecycle_fixture::log_process(\"runner\", std::process::id());",
    )?;
    runner = replace(
        &runner,
        "pub fn main(args: &[String]) -> i32 {",
        "pub fn main(args: &[String]) -> i32 {\n    if args.first().is_some_and(|arg| arg == \"--test-descendant\") {\n        return lifecycle_fixture::descendant();\n    }",
    )?;
    if matches!(fault, "late-start" | "startup-stall") {
        let delay = if fault == "late-start" {
            "Duration::from_millis(1500)"
        } else {
            "Duration::from_secs(20)"
        };
        runner = replace(
            &runner,
            "    let mut child = Command::new(std::env::current_exe()?);",
            &format!(
                "    std::thread::sleep({delay});\n    let mut child = Command::new(std::env::current_exe()?);"
            ),
        )?;
    }
    if matches!(fault, "runner-stall" | "early-failure") {
        let next = if fault == "early-failure" {
            "Ok(7)"
        } else {
            "std::thread::sleep(Duration::from_secs(20));\n    child.wait().map(status_code)"
        };
        runner = replace(
            &runner,
            "    supervise(&mut child)",
            &format!(
                "    let mut child = child.spawn()?;\n    lifecycle_fixture::wait_ready(\"checker-ready\");\n    {next}"
            ),
        )?;
    }
    match control {
        "drain" => {
            runner = replace(
                &runner,
                ".stdout(Stdio::inherit())",
                ".stdout(Stdio::null())",
            )?
        }
        "stderr-drain" => {
            runner = replace(
                &runner,
                ".stderr(Stdio::from(stdout))",
                ".stderr(Stdio::null())",
            )?
        }
        "failclosed" => entry = replace(&entry, "(*) fail 'guard failed'", "(*) exit 0")?,
        "deadline" => entry = replace(&entry, "/bin/sleep 3;", "/bin/sleep 6;")?,
        "cleanup" => {
            entry = replace(
                &entry,
                "kill -KILL -- -\"$pid\" 2>/dev/null",
                ": # ablated runner-group cleanup",
            )?
        }
        _ => {}
    }
    let fixture = replace(
        include_str!("testdata/fault.rs.txt"),
        "FAULT_LITERAL",
        &serde_json::to_string(fault)?,
    )?;
    Ok(FaultPatch {
        runner,
        entry,
        links,
        fixture,
    })
}
pub fn violations(fault: &str, result: &LifecycleResult) -> Vec<String> {
    let mut problems = Vec::new();
    let process = &result.process;
    if process.timed_out || !process.spawn_error.is_empty() || !process.wait_error.is_empty() {
        problems.push("harness process did not complete normally".into());
    }
    if !result.alive_at_return.is_empty() {
        problems.push("descendants alive at return".into());
    }
    if !result.alive_after_poll.is_empty() {
        problems.push("descendants survived cleanup".into());
    }
    let runner = result.processes.iter().rev().find(|p| p.role == "runner");
    if !runner.is_some_and(|r| r.pid == r.pgid && result.processes.iter().all(|p| p.pgid == r.pid))
    {
        problems.push("runner/checker/descendant process groups differ".into());
    }
    match fault {
        "pipe" | "late-start" | "leftover" => {
            if process.status != 0 || !process.stderr.is_empty() {
                problems.push("successful checker failed".into());
            }
            if fault == "pipe" {
                let event: Option<Value> = serde_json::from_str(&process.stdout).ok();
                if !event.is_some_and(|event| {
                    event["event"]["marker"] == "synthetic input" && event["pipe"] == true
                }) {
                    problems.push("event/pipe contract changed".into());
                }
            } else if !process.stdout.is_empty() {
                problems.push("unexpected successful output".into());
            }
            if fault == "late-start"
                && (process.milliseconds <= 1000.0 || process.milliseconds >= 3500.0)
            {
                problems.push("late startup timing changed".into());
            }
        }
        "large-reason" | "large-stderr" => {
            if process.status != 2
                || !process.stdout.is_empty()
                || process.stderr != format!("{}\n", "x".repeat(131072))
                || result.producer_bytes != 131073
            {
                problems.push("large reason/status did not drain byte-exactly".into());
            }
        }
        _ => {
            if process.status != 2
                || !process.stdout.is_empty()
                || !process.stderr.contains("so this call is blocked")
            {
                problems.push("operational failure did not deny".into());
            }
            if matches!(fault, "panic" | "partial-panic")
                && [
                    "synthetic checker panic",
                    "PARTIAL_RESULT_CANARY",
                    "panicked at",
                    "stack backtrace:",
                ]
                .iter()
                .any(|s| process.stderr.contains(s))
            {
                problems.push("checker panic leaked partial output or stack".into());
            }
            if matches!(
                fault,
                "slow-reason" | "hang" | "runner-stall" | "startup-stall"
            ) && (process.milliseconds <= 2500.0 || process.milliseconds >= 3500.0)
            {
                problems.push("three-second total deadline changed".into());
            }
        }
    }
    problems
}
fn living(records: &[ProcessRecord]) -> Result<Vec<ProcessRecord>> {
    records
        .iter()
        .filter_map(|record| match alive(record.pid) {
            Ok(true) => Some(Ok(record.clone())),
            Ok(false) => None,
            Err(e) => Some(Err(e)),
        })
        .collect()
}
pub fn execute_fault(entry: &Path, home: &Path, body: &[u8]) -> Result<LifecycleResult> {
    let mut result = LifecycleResult::default();
    let observed = run_process(
        &[
            entry.to_string_lossy().into(),
            "--runtime".into(),
            "codex".into(),
        ],
        body,
        home,
        &[
            ("HOME".into(), home.to_string_lossy().into()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ],
        Duration::from_secs(8),
        |_| {
            let file = fs::File::open(home.join("processes.jsonl"))?;
            for record in serde_json::Deserializer::from_reader(file).into_iter::<ProcessRecord>() {
                let record = record?;
                if record.pid <= 0 || record.pgid <= 0 {
                    return Err("invalid process identity".into());
                }
                result.processes.push(record);
            }
            result.alive_at_return = living(&result.processes)?;
            result.alive_after_poll = result.alive_at_return.clone();
            let deadline = Instant::now() + Duration::from_millis(500);
            while !result.alive_after_poll.is_empty() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(25));
                result.alive_after_poll = living(&result.processes)?;
            }
            Ok(())
        },
    );
    let cleanup = (|| -> Result<()> {
        for record in &result.processes {
            if let Err(e) = kill_group(record.pgid)
                && e.downcast_ref::<rustix::io::Errno>() != Some(&rustix::io::Errno::PERM)
            {
                return Err(e);
            }
            kill_pid(record.pid)?;
        }
        Ok(())
    })();
    match observed {
        Ok(process) => result.process = process,
        Err(cause) => {
            if let Some(failure) = cause.downcast_ref::<ProcessFailure>() {
                result.process = failure.process.clone();
            }
            return Err(Box::new(LifecycleFailure { result, cause }));
        }
    }
    cleanup?;
    match fs::read_to_string(home.join("producer-bytes")) {
        Ok(bytes) => result.producer_bytes = bytes.parse()?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(result)
}
pub struct LifecycleOptions {
    pub source: PathBuf,
    pub output: PathBuf,
    pub cargo: String,
    pub control: String,
}
pub fn run_lifecycle(options: &LifecycleOptions) -> Result<()> {
    let faults = if options.control.is_empty() {
        FAULTS.to_vec()
    } else {
        vec![
            CONTROLS
                .iter()
                .find(|(name, _)| *name == options.control)
                .ok_or("unknown lifecycle control")?
                .1,
        ]
    };
    let output = new_output(&options.output)?;
    let bindings = source_bindings(&options.source)?;
    let build = output.join("test-build");
    for name in [
        "src",
        "tests",
        "examples",
        "cmd",
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "VERSION",
    ] {
        copy_tree(&options.source.join(name), &build.join(name))?;
    }
    let original = fs::read_to_string(options.source.join("src/entry.rs"))?;
    let entry = fs::read_to_string(options.source.join("bin/agent-guard"))?;
    let links = fs::read_to_string(options.source.join("src/filesystem/links.rs"))?;
    let cargo = look_path(&options.cargo)?;
    let home = PathBuf::from(env::var_os("HOME").ok_or("HOME is required")?);
    let cache = |name, directory| {
        env::var(name).unwrap_or_else(|_| home.join(directory).to_string_lossy().into())
    };
    let target = output.join("cargo-target");
    let environment = vec![
        (
            "PATH".into(),
            format!(
                "{}:/usr/bin:/bin",
                cargo.parent().ok_or("missing cargo parent")?.display()
            ),
        ),
        ("HOME".into(), build.to_string_lossy().into()),
        ("TMPDIR".into(), output.to_string_lossy().into()),
        ("CARGO_NET_OFFLINE".into(), "true".into()),
        ("CARGO_HOME".into(), cache("CARGO_HOME", ".cargo")),
        ("RUSTUP_HOME".into(), cache("RUSTUP_HOME", ".rustup")),
        ("CARGO_TARGET_DIR".into(), target.to_string_lossy().into()),
    ];
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(output.join("results.jsonl"))?;
    let (mut count, mut failed) = (0, 0);
    for fault in faults {
        let patch = inject_fault(&original, &entry, &links, fault, &options.control)?;
        for (name, text) in [
            ("src/entry.rs", &patch.runner),
            ("src/entry/lifecycle_fixture.rs", &patch.fixture),
            ("src/filesystem/links.rs", &patch.links),
        ] {
            create_private_dirs(build.join(name).parent().ok_or("missing fault parent")?)?;
            write_file(&build.join(name), text.as_bytes(), 0o600)?;
        }
        let binary = output.join(format!("fault-{fault}"));
        let compiled = run(
            &[
                cargo.to_string_lossy().into(),
                "build".into(),
                "--locked".into(),
                "--release".into(),
                "--bin".into(),
                "agent-guard-native".into(),
            ],
            &[],
            &build,
            &environment,
            Duration::ZERO,
        )?;
        write_json(&output.join(format!("{fault}-build.json")), &compiled)?;
        if compiled.status != 0 || compiled.timed_out {
            return Err(format!(
                "fault build failed: {} {}",
                compiled.spawn_error, compiled.stderr
            )
            .into());
        }
        copy_file(&target.join("release/agent-guard-native"), &binary)?;
        let binding = hash_file(&binary, "agent-guard-native")?;
        if matches!(fault, "large-reason" | "large-stderr") {
            let home = temporary(&output, "producer-control-")?;
            let producer = run(
                &[binary.to_string_lossy().into(), "--checker".into()],
                b"{}",
                &home,
                &[
                    ("HOME".into(), home.to_string_lossy().into()),
                    ("PATH".into(), "/usr/bin:/bin".into()),
                ],
                Duration::from_secs(8),
            )?;
            let payload = if fault == "large-stderr" {
                &producer.stderr
            } else {
                &producer.stdout
            };
            let written = fs::read_to_string(home.join("producer-bytes"))?;
            write_json(&output.join(format!("{fault}-producer.json")), &producer)?;
            if producer.status != 2
                || payload != &format!("{}\n", "x".repeat(131072))
                || written != "131073"
            {
                return Err("independent producer control failed".into());
            }
        }
        for repeat in 0..3 {
            let home = temporary(&output, &format!("{fault}-{repeat}-"))?;
            let bin = home.join("package/bin");
            create_private_dirs(&bin)?;
            copy_file(&binary, &bin.join("agent-guard-native"))?;
            write_file(&bin.join("agent-guard"), patch.entry.as_bytes(), 0o700)?;
            let body: &[u8] = if fault == "dependency-failure" {
                br#"{"tool_name":"Bash","tool_input":{"command":"true"}}"#
            } else {
                br#"{"marker":"synthetic input"}"#
            };
            let result = match execute_fault(&bin.join("agent-guard"), &home, body) {
                Ok(result) => result,
                Err(e) => {
                    if let Some(failure) = e.downcast_ref::<LifecycleFailure>() {
                        write_json(
                            &output.join(format!("{fault}-{repeat}-error.json")),
                            &failure.result,
                        )?;
                    }
                    return Err(e);
                }
            };
            let problems = violations(fault, &result);
            count += 1;
            if !problems.is_empty() {
                failed += 1;
            }
            let mut row = serde_json::to_value(&result)?;
            row["fault"] = json!(fault);
            row["repeat"] = json!(repeat);
            row["violations"] = json!(problems);
            row["binary"] = serde_json::to_value(&binding)?;
            json_line(&mut file, &row)?;
            println!(
                "{fault} {repeat} status={} ms={:.1} violations=[{}]",
                result.process.status,
                result.process.milliseconds,
                problems.join(" ")
            );
        }
    }
    write_json(
        &output.join("summary.json"),
        &json!({"cases": count, "failed_cases": failed, "control": options.control, "source": bindings}),
    )?;
    if failed > 0 {
        return Err(format!("{failed} lifecycle cases violated their contract").into());
    }
    Ok(())
}
