#[path = "harness/mod.rs"]
pub mod harness;

use harness::{lifecycle::*, model::*, process::*, runtime::*, runtime_driver::*};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn scratch(name: &str) -> PathBuf {
    let parent = Path::new(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(parent).unwrap();
    temporary(parent, name).unwrap()
}
fn synthetic_env(home: &Path) -> Vec<(String, String)> {
    vec![
        ("HOME".into(), home.to_string_lossy().into()),
        ("PATH".into(), "/usr/bin:/bin".into()),
    ]
}
fn hook(status: i32, stderr: &str) -> HookObservation {
    HookObservation {
        process: ProcessResult {
            status,
            stderr: stderr.into(),
            ..ProcessResult::default()
        },
        input: json!({"tool_input":{"command":CORPUS[0].command}}),
        survivors: vec![],
        observation_error: String::new(),
    }
}
#[test]
fn runtime_verdict_requires_matching_hook_and_execution_evidence() {
    for (status, text, expected, conflict) in [
        (
            0,
            Some("Command blocked by PreToolUse hook: cat PUBLIC_CANARY"),
            "deny",
            true,
        ),
        (2, Some("PUBLIC_CANARY"), "allow", true),
        (2, Some("DENIED: synthetic"), "deny", false),
        (2, None, "unverified", false),
    ] {
        let result = text.map(|text| ToolResult {
            text: text.into(),
            is_error: false,
            raw: Value::Null,
        });
        let verdict = runtime_verdict(
            "codex",
            &[hook(status, "DENIED: synthetic")],
            result.as_ref(),
            &CORPUS[0],
        );
        assert_eq!(
            (verdict.runtime.as_str(), verdict.conflict),
            (expected, conflict)
        );
    }
    let result = ToolResult {
        text: "PUBLIC_CANARY".into(),
        is_error: false,
        raw: Value::Null,
    };
    assert_eq!(
        runtime_verdict("claude", &[], Some(&result), &CORPUS[0]).runtime,
        "unverified"
    );
    let mut wrong = hook(0, "");
    wrong.input["tool_input"]["command"] = json!("other");
    assert_eq!(
        runtime_verdict("claude", &[wrong], Some(&result), &CORPUS[0]).runtime,
        "unverified"
    );
}
#[test]
fn hook_observation_preserves_input_streams_and_failure_status() {
    let home = scratch("observation-");
    let entry = home.join("entry");
    let input = b" { \"tool_name\": \"Bash\", \"tool_input\": {\"command\": \"printf public\"}, \"sentinel\": [1, true] }\n";
    for status in [0, 2, 7] {
        write_file(&entry,format!("#!/bin/sh\ncat > forwarded\nprintf 'synthetic advice\\n'\nprintf 'synthetic reason\\n' >&2\nexit {status}\n").as_bytes(),0o700).unwrap();
        let result =
            observe_hook("claude", &entry, &home, input, &synthetic_env(&home), false).unwrap();
        assert_eq!(result.process.status, status);
        assert_eq!(result.process.stdout, "synthetic advice\n");
        assert_eq!(result.process.stderr, "synthetic reason\n");
        assert!(result.survivors.is_empty());
        assert!(result.observation_error.is_empty());
        assert_eq!(fs::read(home.join("forwarded")).unwrap(), input);
        assert_eq!(
            result.input,
            serde_json::from_slice::<Value>(input).unwrap()
        );
    }
    let result = observe_hook(
        "codex",
        &home.join("missing"),
        &home,
        b"{}",
        &synthetic_env(&home),
        false,
    )
    .unwrap();
    assert!(!result.process.spawn_error.is_empty());
    assert_ne!(result.process.status, 0);
    fs::remove_dir_all(home).unwrap();
}
#[test]
fn setup_process_without_deadline_completes() {
    let home = scratch("setup-");
    let result = run(
        &["/bin/sh".into(), "-c".into(), "printf public".into()],
        &[],
        &home,
        &synthetic_env(&home),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(result.status, 0);
    assert_eq!(result.stdout, "public");
    assert!(!result.timed_out);
    assert!(result.spawn_error.is_empty() && result.wait_error.is_empty());
    fs::remove_dir_all(home).unwrap();
}
#[test]
fn timeout_reaps_group_and_inherited_pipes_are_bounded() {
    let home = scratch("timeout-");
    let result = run(
        &[
            "/bin/sh".into(),
            "-c".into(),
            "sleep 20 & child=$!; printf '%s' \"$child\" > child; wait".into(),
        ],
        &[],
        &home,
        &synthetic_env(&home),
        Duration::from_millis(150),
    )
    .unwrap();
    assert!(result.timed_out);
    assert_ne!(result.status, 0);
    assert!(result.milliseconds < 2000.0);
    let pid: i32 = fs::read_to_string(home.join("child"))
        .unwrap()
        .parse()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(1);
    while alive(pid).unwrap() {
        assert!(Instant::now() < until, "child survived timeout");
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::remove_dir_all(home).unwrap();
}
#[test]
fn output_rejects_checkouts_aliases_protected_paths_and_existing_evidence() {
    let home = scratch("boundary-");
    let checkout = home.join("checkout");
    fs::create_dir_all(checkout.join(".git")).unwrap();
    symlink(&checkout, home.join("alias")).unwrap();
    for path in [checkout.join("report"), home.join("alias/report")] {
        assert!(new_output(&path).is_err());
    }
    let evidence = home.join("evidence");
    new_output(&evidence).unwrap();
    assert!(new_output(&evidence).is_err());
    let protected =
        PathBuf::from(std::env::var_os("HOME").unwrap()).join("Library/Containers/port-test");
    symlink(&protected, home.join("protected-alias")).unwrap();
    assert!(outside_path(&home.join("protected-alias/out")).is_err());
    fs::remove_dir_all(home).unwrap();
}
#[test]
fn lifecycle_rejects_panic_and_partial_output_leaks() {
    for fault in ["panic", "partial-panic"] {
        for trace in [
            "",
            "thread 'main' panicked at fixture.rs:1",
            "stack backtrace:",
            "PARTIAL_RESULT_CANARY",
        ] {
            let result = LifecycleResult {
                process: ProcessResult {
                    status: 2,
                    stderr: format!("agent-guard failed, so this call is blocked\n{trace}"),
                    ..ProcessResult::default()
                },
                processes: vec![ProcessRecord {
                    role: "runner".into(),
                    pid: 100,
                    pgid: 100,
                }],
                ..LifecycleResult::default()
            };
            let violations = violations(fault, &result);
            assert_eq!(
                violations
                    .iter()
                    .any(|p| p.contains("checker panic leaked")),
                !trace.is_empty()
            );
            if trace.is_empty() {
                assert!(violations.is_empty());
            }
        }
    }
}
#[test]
fn scripted_models_preserve_messages_and_responses_protocols() {
    for runtime in ["claude", "pi", "codex"] {
        let mut model = ScriptedModel::default();
        model.begin(runtime, "printf public");
        let request = if runtime == "codex" {
            json!({"model":"synthetic","input":[]})
        } else {
            json!({"model":"synthetic","stream":true,"messages":[]})
        };
        let (_, body) = model
            .response(
                "POST",
                if runtime == "codex" {
                    "/v1/responses"
                } else {
                    "/v1/messages"
                },
                request.to_string().as_bytes(),
            )
            .unwrap();
        assert!(body.contains(if runtime == "codex" {
            "exec_command"
        } else if runtime == "claude" {
            "Bash"
        } else {
            "bash"
        }));
        assert!(body.contains("printf public"));
        let result = if runtime == "codex" {
            json!({"model":"synthetic","input":[{"type":"function_call_output","output":"public"}]})
        } else {
            json!({"model":"synthetic","stream":false,"messages":[{"content":[{"type":"tool_result","is_error":true,"content":[{"text":"pub"},{"text":"lic"}]}]}]})
        };
        let (_, body) = model
            .response(
                "POST",
                if runtime == "codex" {
                    "/v1/responses"
                } else {
                    "/v1/messages"
                },
                result.to_string().as_bytes(),
            )
            .unwrap();
        assert!(body.contains("done"));
        assert_eq!(model.result.as_ref().unwrap().text, "public");
        assert_eq!(model.result.as_ref().unwrap().is_error, runtime != "codex");
    }
}

#[path = "harness/runtime_tests.rs"]
mod runtime_tests;

#[test]
fn hook_cli_forwards_bytes_and_errors_without_success_coercion() {
    let home = scratch("cli-hook-");
    let entry = home.join("guard");
    let trace = home.join("trace.jsonl");
    write_file(
        &entry,
        b"#!/bin/sh\ncat > forwarded\nprintf 'advice\\n'\nprintf 'reason\\n' >&2\nexit 2\n",
        0o700,
    )
    .unwrap();
    let input = b" { \"tool_input\": {\"command\": \"public\"} }\n";
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-guard-runtime"));
    command
        .args(["--hook", "pi"])
        .current_dir(&home)
        .env("AGENT_GUARD_TEST_ENTRY", &entry)
        .env("AGENT_GUARD_TEST_HOOK_TRACE", &trace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output.stdout, b"advice\n");
    assert_eq!(output.stderr, b"reason\n");
    assert_eq!(fs::read(home.join("forwarded")).unwrap(), input);
    assert_eq!(read_hooks(&trace).unwrap().len(), 1);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn simultaneous_large_input_and_output_do_not_deadlock() {
    let home = scratch("duplex-");
    let input = vec![b'i'; 262144];
    let result = run(
        &["/bin/sh".into(), "-c".into(), "dd if=/dev/zero bs=131072 count=1 2>/dev/null; cat > forwarded; printf 'done' >&2; exit 2".into()],
        &input, &home, &synthetic_env(&home), Duration::from_secs(2),
    ).unwrap();
    assert_eq!(result.status, 2);
    assert!(!result.timed_out);
    assert_eq!(result.stdout.as_bytes(), vec![0; 131072]);
    assert_eq!(result.stderr, "done");
    assert_eq!(fs::read(home.join("forwarded")).unwrap(), input);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn observer_failure_retains_completed_child_output_and_reaps_it() {
    let home = scratch("observer-error-");
    let failure = run_process(
        &[
            "/bin/sh".into(),
            "-c".into(),
            "printf public; printf reason >&2; exit 2".into(),
        ],
        &[],
        &home,
        &synthetic_env(&home),
        Duration::from_secs(2),
        |_| Err("synthetic observer failure".into()),
    )
    .unwrap_err();
    let failure = failure.downcast_ref::<ProcessFailure>().unwrap();
    assert_eq!(failure.process.status, 2);
    assert_eq!(failure.process.stdout, "public");
    assert_eq!(failure.process.stderr, "reason");
    assert!(failure.process.milliseconds > 0.0);
    assert!(!alive(failure.process.pid).unwrap());
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn lifecycle_observation_error_retains_the_failed_attempt() {
    let home = scratch("lifecycle-error-");
    let entry = home.join("entry");
    write_file(
        &entry,
        b"#!/bin/sh\nprintf 'synthetic failure' >&2; exit 2\n",
        0o700,
    )
    .unwrap();
    let error = execute_fault(&entry, &home, b"{}").unwrap_err();
    let failure = error.downcast_ref::<LifecycleFailure>().unwrap();
    assert_eq!(failure.result.process.status, 2);
    assert_eq!(failure.result.process.stderr, "synthetic failure");
    assert!(failure.result.processes.is_empty());
    assert!(!alive(failure.result.process.pid).unwrap());
    fs::remove_dir_all(home).unwrap();
}
