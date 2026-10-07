use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn native_limits_are_completed_refusals() {
    let fixture = crate::support::Fixture::new();
    let event = |command: String, cwd: &str| {
        serde_json::to_vec(
            &serde_json::json!({"tool_name":"Bash", "tool_input":{"command":command}, "cwd":cwd}),
        )
        .unwrap()
    };
    let cases = [
        (event("true".into(), "relative"), "absolute cwd", 2),
        (
            event(
                "printf public".repeat(agent_guard_rust::limits::MAX_INPUT_BYTES),
                &fixture.project,
            ),
            "input byte limit",
            2,
        ),
        (
            event(
                format!("{}true{}", "(".repeat(65), ")".repeat(65)),
                &fixture.project,
            ),
            "nesting",
            2,
        ),
        (vec![0xff], "UTF-8", 2),
        (
            event(format!("printf '{}'", "p".repeat(80_000)), &fixture.project),
            "",
            0,
        ),
    ];
    for consumer in ["claude", "codex", "pi"] {
        for mode in ["--checker", ""] {
            for (bytes, reason, status) in &cases {
                let mut command = Command::new(env!("CARGO_BIN_EXE_agent-guard-rust-slice"));
                if !mode.is_empty() {
                    command.arg(mode);
                }
                let mut child = command
                    .args(["--runtime", consumer, "--cwd"])
                    .arg(&fixture.project)
                    .env("HOME", &fixture.home)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                child.stdin.take().unwrap().write_all(bytes).unwrap();
                let output = child.wait_with_output().unwrap();
                assert_eq!(
                    output.status.code(),
                    Some(if mode.is_empty() && *status == 2 {
                        3
                    } else {
                        *status
                    }),
                    "{consumer} {mode} {reason}"
                );
                let text = String::from_utf8([output.stdout, output.stderr].concat()).unwrap();
                if *status == 0 {
                    assert!(text.is_empty());
                } else {
                    assert!(text.contains(reason), "{consumer} {mode}: {text}");
                    assert!(text.contains("recheck"));
                    assert!(!text.contains("repair the failed check"));
                }
            }
        }
    }
}
