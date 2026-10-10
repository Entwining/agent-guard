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

fn command(form: &str, depth: usize, basename: &str) -> String {
    match form {
        "literal" => format!("cat {}{basename}", "p/".repeat(depth)),
        "parameter-prefix" => format!("cat $PWD/{}{basename}", "p/".repeat(depth)),
        // A fixed value keeps the expanded depth equal to `depth`; `$PWD` would multiply it by the host's temporary root.
        "repeated-parameter" => format!("p=p/; cat {}{basename}", "${p}".repeat(depth)),
        _ => panic!("unknown deep-path form {form}"),
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
            for (form, row) in contract["forms"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|form| {
                    contract["rows"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(move |row| (form.as_str().unwrap(), row))
                })
            {
                let name = row["basename"].as_str().unwrap();
                let class = row["expected"].as_str().unwrap();
                let body = fixture
                    .body(&json!({"tool":"Bash","input":{"command":command(form, depth, name)}}));
                let result = evaluate(Event {
                    bytes: &body,
                    context: &context,
                    probe: &mut NoLinks,
                });
                assert_eq!(
                    support::class(&result),
                    class,
                    "depth={depth} {consumer} {form} {name}"
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
    let contract: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/rust-deep-path-cost.json")).unwrap();
    for (consumer, form) in ["claude", "codex", "pi"].into_iter().flat_map(|consumer| {
        contract["forms"]
            .as_array()
            .unwrap()
            .iter()
            .map(move |form| (consumer, form.as_str().unwrap()))
    }) {
        let body = fixture
            .body(&json!({"tool":"Bash","input":{"command":command(form, 20_000, "file.txt")}}));
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
            "{consumer} {form}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty() && output.stderr.is_empty());
    }
}
