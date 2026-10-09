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

mod driver;
pub use driver::run_lifecycle;
