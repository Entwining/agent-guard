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

fn fixtures(home: &Path, guard: &[u8]) -> (PathBuf, PathBuf) {
    let bin = home.join("clients");
    fs::create_dir(&bin).unwrap();
    let helper = env!("CARGO_BIN_EXE_agent-guard-harness-client");
    for runtime in ["claude", "pi", "codex"] {
        #[expect(
            clippy::disallowed_methods,
            reason = "This fixture resolves only its synthetic client directory to test executable aliases."
        )]
        let client = if runtime == "codex" {
            fs::canonicalize(&bin).unwrap().join("installed-codex")
        } else {
            bin.join(runtime)
        };
        let check = if runtime == "codex" {
            format!(
                "test \"$0\" = {} || exit 80\n",
                quote(&client.to_string_lossy())
            )
        } else {
            String::new()
        };
        write_file(&client,format!("#!/bin/sh\n{check}if test \"$1\" = --version; then printf 'synthetic runtime 1\\n'; exit 0; fi\nexec {} {runtime} \"$@\"\n",quote(helper)).as_bytes(),0o700).unwrap();
        if runtime == "codex" {
            symlink(&client, bin.join(runtime)).unwrap();
        }
    }
    let package = home.join("package/bin");
    fs::create_dir_all(&package).unwrap();
    let entry = package.join("agent-guard");
    write_file(&entry, guard, 0o700).unwrap();
    write_file(
        &package.join("agent-guard-native"),
        b"synthetic binding only",
        0o700,
    )
    .unwrap();
    (bin, entry)
}
fn driver(
    bin: &Path,
    entry: &Path,
    output: &Path,
    ablate: bool,
    runtimes: &str,
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-guard-runtime"));
    command
        .args(["--source", env!("CARGO_MANIFEST_DIR"), "--entry"])
        .arg(entry)
        .arg("--output")
        .arg(output)
        .args(["--runtimes", runtimes])
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()));
    if ablate {
        command.arg("--ablate");
    }
    command.output().unwrap()
}
#[test]
fn runtime_driver_observes_all_cases_bindings_controls_and_missing_hooks() {
    let home = scratch("driver-");
    let (bin,entry) = fixtures(&home,b"#!/bin/sh\nbody=$(cat)\ncase \"$body\" in *Library*|*.ssh*|*find*) printf 'DENIED: synthetic rule\\n' >&2; exit 2;; esac\n");
    for name in ["missing", "other"] {
        let selected = entry.with_file_name(name);
        if name == "other" {
            write_file(&selected, b"#!/bin/sh\n", 0o700).unwrap();
        }
        let directory = home.join(format!("invalid-{name}"));
        assert!(
            !driver(&bin, &selected, &directory, false, "codex")
                .status
                .success()
        );
        assert!(!directory.exists());
    }
    let alias = home.join("entry-alias");
    symlink(&entry, &alias).unwrap();
    for ablate in [false, true] {
        let directory = home.join(format!("report-{ablate}"));
        let output = driver(
            &bin,
            if ablate { &entry } else { &alias },
            &directory,
            ablate,
            "claude,pi,codex",
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value =
            serde_json::from_slice(&fs::read(directory.join("report.json")).unwrap()).unwrap();
        let rows: Vec<RuntimeRow> = serde_json::from_value(report["records"].clone()).unwrap();
        let summaries: Vec<RuntimeSummary> =
            serde_json::from_value(report["summary"].clone()).unwrap();
        let mut expected = BTreeSet::new();
        for runtime in ["claude", "pi", "codex"] {
            for run in 0..3 {
                for case in CORPUS {
                    expected.insert((runtime.to_owned(), run, case.id.to_owned()));
                }
            }
        }
        for row in rows {
            assert!(
                expected.remove(&(row.runtime, row.run, row.id.clone())),
                "duplicate or unknown case"
            );
            let case = CORPUS.iter().find(|case| case.id == row.id).unwrap();
            assert_eq!(row.command, case.command);
            assert_eq!(row.expected, case.expected);
            assert_eq!(
                row.verdict.runtime,
                if ablate { "allow" } else { case.expected }
            );
            assert_eq!(row.hooks.len(), 1);
            assert!(row.result.is_some());
            assert!(!row.verdict.conflict);
        }
        assert!(expected.is_empty());
        assert_eq!(summaries.len(), 3);
        for summary in summaries {
            assert!(summary.complete);
            assert_eq!(summary.verified, CORPUS.len() * 3);
            assert_eq!(summary.matched, CORPUS.len() * 3);
        }
        let bindings: Vec<Binding> = serde_json::from_value(report["manifest"].clone()).unwrap();
        assert_eq!(bindings.len(), 2);
        let mut names = BTreeSet::new();
        for binding in bindings {
            assert!(names.insert(binding.path.clone()));
            let expected = hash_file(
                &entry
                    .parent()
                    .unwrap()
                    .join(Path::new(&binding.path).file_name().unwrap()),
                &binding.path,
            )
            .unwrap();
            assert_eq!(
                (binding.sha256, binding.mode),
                (expected.sha256, expected.mode)
            );
        }
        assert_eq!(
            names,
            BTreeSet::from(["bin/agent-guard".into(), "bin/agent-guard-native".into()])
        );
        let clients: Vec<RuntimeClient> =
            serde_json::from_value(report["clients"].clone()).unwrap();
        assert_eq!(clients.len(), 3);
        let mut seen = BTreeSet::new();
        for client in clients {
            assert!(seen.insert(client.runtime.clone()));
            #[expect(
                clippy::disallowed_methods,
                reason = "This assertion resolves a synthetic client alias to verify the recorded executable identity."
            )]
            let path = fs::canonicalize(bin.join(&client.runtime)).unwrap();
            let binding = hash_file(&path, &path.to_string_lossy()).unwrap();
            assert_eq!(
                (
                    client.executable.path,
                    client.executable.sha256,
                    client.executable.mode
                ),
                (binding.path, binding.sha256, binding.mode)
            );
            assert_eq!(client.version.status, 0);
            assert_eq!(client.version.stdout, "synthetic runtime 1\n");
        }
    }
    write_file(
        &bin.join("codex"),
        b"#!/bin/sh\nprintf 'SYNTHETIC_RUNTIME_CLIENT\\n'\nexit 7\n",
        0o700,
    )
    .unwrap();
    let directory = home.join("missing-hook");
    assert!(
        !driver(&bin, &entry, &directory, false, "codex")
            .status
            .success()
    );
    let report: Value =
        serde_json::from_slice(&fs::read(directory.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["records"].as_array().unwrap().len(), 1);
    assert_eq!(report["records"][0]["id"], CORPUS[0].id);
    assert_eq!(report["records"][0]["runtime"], "codex");
    assert_eq!(report["records"][0]["verdict"], "unverified");
    assert_eq!(report["summary"][0]["plannedCalls"], CORPUS.len() * 3);
    assert_eq!(report["summary"][0]["verified"], 0);
    assert!(
        report["summary"][0]["falseAllow"].is_null() && report["summary"][0]["falseDeny"].is_null()
    );
    fs::remove_dir_all(home).unwrap();
}
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
