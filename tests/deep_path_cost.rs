mod support;

use agent_guard_rust::{Event, adapters, evaluate};
use serde_json::json;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

struct NoLinks;
impl agent_guard_rust::filesystem::Probe for NoLinks {
    fn read_link(&mut self, _: &Path) -> std::io::Result<Option<PathBuf>> {
        Ok(None)
    }
    fn stat(
        &mut self,
        _: &Path,
    ) -> std::io::Result<Option<agent_guard_rust::filesystem::Metadata>> {
        panic!("deep public paths must not need stat");
    }
}

#[test]
fn deep_literal_paths_preserve_public_and_protected_results() {
    let fixture = support::Fixture::new();
    let contract: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/rust-deep-path-cost.json")).unwrap();
    for depth in contract["depths"].as_array().unwrap() {
        let depth = depth.as_u64().unwrap() as usize;
        for consumer in ["claude", "codex", "pi"] {
            let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
            for row in contract["rows"].as_array().unwrap() {
                let name = row["basename"].as_str().unwrap();
                let class = row["expected"].as_str().unwrap();
                let body = fixture.body(&json!({"tool":"Bash","input":{"command":format!("cat {}{name}", "p/".repeat(depth))}}));
                let result = evaluate(Event {
                    bytes: &body,
                    context: &context,
                    probe: &mut NoLinks,
                });
                assert_eq!(
                    support::class(&result),
                    class,
                    "depth={depth} {consumer} {name}"
                );
                let wire = adapters::render(context.consumer, &result);
                assert_eq!(wire.exit, if class == "D" { 2 } else { 0 });
                assert!(wire.stdout.is_empty());
                if class == "D" {
                    for field in ["reason", "alternative"] {
                        assert!(wire.stderr.contains(row[field].as_str().unwrap()));
                    }
                } else {
                    assert!(wire.stderr.is_empty());
                }
            }
        }
    }
}

#[test]
fn native_checker_completes_twenty_thousand_public_components() {
    let fixture = support::Fixture::new();
    for consumer in ["claude", "codex", "pi"] {
        let body = fixture.body(&json!({"tool":"Bash","input":{"command":format!("cat {}file.txt", "p/".repeat(20_000))}}));
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
        assert_eq!(
            output.status.code(),
            Some(0),
            "{consumer}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty() && output.stderr.is_empty());
    }
}
