mod support;

use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn native_checker_completes_sixteen_thousand_operands_without_losing_protection() {
    let fixture = support::Fixture::new();
    let packet: Value = serde_json::from_str(include_str!("fixtures/rust-wide-argv.json")).unwrap();
    let width = packet["width"].as_u64().unwrap() as usize;
    let words = vec!["pub.txt"; width].join(" ");
    for row in packet["rows"].as_array().unwrap() {
        let source = row["command"].as_str().unwrap().replace("$WORDS", &words);
        let body = fixture.body(&json!({"tool":"Bash","input":{"command":source}}));
        for consumer in ["claude", "codex", "pi"] {
            let mut child = Command::new(env!("CARGO_BIN_EXE_agent-guard-native"))
                .args(["--checker", "--runtime", consumer, "--cwd"])
                .arg(&fixture.project)
                .env("HOME", &fixture.home)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(&body).unwrap();
            let output = child.wait_with_output().unwrap();
            let denied = row["expected"] == "D";
            assert_eq!(
                output.status.code(),
                Some(if denied { 2 } else { 0 }),
                "{consumer}: {}: {}",
                row["id"],
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.stdout.is_empty(), "{}: unexpected advice", row["id"]);
            if denied {
                let stderr = String::from_utf8(output.stderr).unwrap();
                assert!(stderr.contains(row["reason"].as_str().unwrap()), "{stderr}");
                assert!(
                    stderr.contains(row["alternative"].as_str().unwrap()),
                    "{stderr}"
                );
            } else {
                assert!(output.stderr.is_empty());
            }
        }
    }
}
