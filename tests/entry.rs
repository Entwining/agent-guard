use std::process::Command;

#[test]
fn version_reports_the_release_owner() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-guard-rust-slice"))
        .arg("--version")
        .output()
        .expect("offline binary should start");
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        format!("agent-guard {}\n", include_str!("../VERSION").trim()).as_bytes()
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn unsupported_invocations_fail_without_stdout() {
    for arguments in [vec!["--checker"], vec!["--version", "extra"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-guard-rust-slice"))
            .args(&arguments)
            .output()
            .expect("offline binary should start");
        assert!(!output.status.success(), "arguments: {arguments:?}");
        assert!(output.stdout.is_empty(), "arguments: {arguments:?}");
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn entry_modes_check_events_and_separate_rejections_from_faults() {
    use std::{fs, io::Write, process::Stdio};
    let home = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("entry-{}", std::process::id()));
    fs::create_dir_all(&home).unwrap();
    for consumer in ["claude", "codex", "pi"] {
        for mode in ["--checker", "--supervised-checker", ""] {
            for (event, check_status) in [
                (r#"{"tool_name":"Bash","tool_input":{"command":"true"}}"#, 0),
                (
                    r#"{"tool_name":"Bash","tool_input":{"command":"printenv"}}"#,
                    2,
                ),
                ("{", 1),
            ] {
                let mut command = Command::new(env!("CARGO_BIN_EXE_agent-guard-rust-slice"));
                if !mode.is_empty() {
                    command.arg(mode);
                }
                let mut child = command
                    .args(["--runtime", consumer, "--cwd"])
                    .arg(&home)
                    .env("HOME", &home)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(event.as_bytes())
                    .unwrap();
                let output = child.wait_with_output().unwrap();
                let expected = if mode != "--checker" && check_status == 2 {
                    3
                } else {
                    check_status
                };
                assert_eq!(output.status.code(), Some(expected), "{consumer} {mode}");
                if check_status == 0 {
                    assert!(output.stdout.is_empty() && output.stderr.is_empty());
                } else {
                    assert!(![output.stdout, output.stderr].concat().is_empty());
                }
            }
        }
    }
    fs::remove_dir_all(home).unwrap();
}
