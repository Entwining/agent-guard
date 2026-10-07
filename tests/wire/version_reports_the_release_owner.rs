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
        format!("agent-guard {}\n", include_str!("../../VERSION").trim()).as_bytes()
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
        assert_eq!(output.status.code(), Some(2), "arguments: {arguments:?}");
        assert!(output.stdout.is_empty(), "arguments: {arguments:?}");
        assert_eq!(
            output.stderr,
            b"usage: agent-guard --runtime claude|codex|pi < event.json\n"
        );
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

#[test]
fn native_protocol_uses_file_path_for_every_consumer() {
    use std::{fs, io::Write, process::Stdio};
    let home = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("native-fields-{}", std::process::id()));
    fs::create_dir_all(&home).unwrap();
    for consumer in ["claude", "codex", "pi"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agent-guard-rust-slice"))
            .args(["--checker", "--runtime", consumer, "--cwd"])
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
            .write_all(br#"{"tool_name":"Read","tool_input":{"file_path":"public"}}"#)
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{consumer}");
        assert!(output.stdout.is_empty() && output.stderr.is_empty());
    }
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn native_advice_matches_go_text_and_json_bytes() {
    use std::{fs, io::Write, process::Stdio};
    let home = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("native-advice-{}", std::process::id()));
    fs::create_dir_all(&home).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-guard-rust-slice"))
        .args(["--checker", "--runtime", "claude", "--cwd"])
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
        .write_all(
            br#"{"tool_name":"Bash","tool_input":{"command":"rg -r replacement needle ./public"}}"#,
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",\"additionalContext\":\"rg -r means --replace. Drop -r; use -n for line numbers, or spell --replace VALUE for an intentional replacement.\"}}\n");
    assert!(output.stderr.is_empty());
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn native_protocol_does_not_fall_through_to_raw_consumer_envelopes() {
    use std::{fs, io::Write, process::Stdio};
    let home = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("native-schema-{}", std::process::id()));
    fs::create_dir_all(&home).unwrap();
    for (consumer, event, expected) in [
        (
            "codex",
            r#"{"name":"exec_command","arguments":{"cmd":"true"}}"#,
            1,
        ),
        ("pi", r#"{"toolName":"bash","input":{"command":"true"}}"#, 1),
        ("claude", r#"{"tool_input":{"command":"true"}}"#, 0),
        ("claude", r#"{"tool_name":"Outside","tool_input":null}"#, 1),
        (
            "codex",
            r#"{"tool_name":"exec_command","tool_input":{"cmd":"("}}"#,
            0,
        ),
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agent-guard-rust-slice"))
            .args(["--checker", "--runtime", consumer, "--cwd"])
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
        assert_eq!(output.status.code(), Some(expected), "{consumer} {event}");
    }
    fs::remove_dir_all(home).unwrap();
}
