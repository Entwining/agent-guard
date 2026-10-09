mod support;

use agent_guard_rust::{Event, adapters, evaluate};
use serde_json::{Value, json};

struct StatFault<'a>(&'a mut support::RecordingProbe);

impl agent_guard_rust::filesystem::Probe for StatFault<'_> {
    fn read_link(&mut self, path: &std::path::Path) -> std::io::Result<Option<std::path::PathBuf>> {
        agent_guard_rust::filesystem::Probe::read_link(self.0, path)
    }
    fn stat(
        &mut self,
        path: &std::path::Path,
    ) -> std::io::Result<Option<agent_guard_rust::filesystem::Metadata>> {
        self.0.stat_calls.push(path.to_str().unwrap().to_owned());
        Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
    }
}

fn partition(name: &str) {
    let packet: Value =
        serde_json::from_str(include_str!("fixtures/rust-fs-dispatch.json")).unwrap();
    let mut consumed = 0;
    let mut evaluated = 0;
    for row in packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["partition"] == name)
    {
        consumed += 1;
        let fixture = support::Fixture::new();
        fixture.setup(row);
        for consumer in ["claude", "codex", "pi"] {
            if row["consumer"]
                .as_str()
                .is_some_and(|selected| selected != consumer)
            {
                continue;
            }
            evaluated += 1;
            let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
            let mut event: Value = if let Some(event) = row.get("event") {
                fixture.expand_value(event)
            } else {
                serde_json::from_slice(&fixture.body(row)).unwrap()
            };
            if consumer == "pi"
                && let Some(path) = event["tool_input"].get("file_path").cloned()
            {
                event["tool_input"]["path"] = path;
                event["tool_input"]
                    .as_object_mut()
                    .unwrap()
                    .remove("file_path");
            }
            let body = serde_json::to_vec(&event).unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = if row["expired_deadline"] == true {
                agent_guard_rust::evaluate_with_deadline(
                    Event {
                        bytes: &body,
                        context: &context,
                        probe: &mut probe,
                    },
                    std::time::Instant::now(),
                )
            } else if row["stat_fault"] == true {
                evaluate(Event {
                    bytes: &body,
                    context: &context,
                    probe: &mut StatFault(&mut probe),
                })
            } else {
                evaluate(Event {
                    bytes: &body,
                    context: &context,
                    probe: &mut probe,
                })
            };
            if name == "aliases" {
                for path in probe.calls.iter().chain(&probe.stat_calls) {
                    assert!(
                        !["/.nofollow", "/.resolve", "/.vol"]
                            .iter()
                            .any(|alias| path.starts_with(alias)),
                        "alias reached probe: {path}"
                    );
                }
            }
            if name == "descriptors" {
                assert!(
                    !probe
                        .calls
                        .iter()
                        .chain(&probe.stat_calls)
                        .any(|path| path.starts_with("/dev/fd")),
                    "descriptor reached probe"
                );
            }
            assert_eq!(
                support::class(&result),
                row["expected"],
                "{consumer} {}: {result:?}",
                row["id"]
            );
            let wire = adapters::render(context.consumer, &result);
            assert_eq!(
                wire.exit,
                if matches!(row["expected"].as_str(), Some("D" | "UR" | "F")) {
                    2
                } else {
                    0
                },
                "{}",
                row["id"]
            );
            assert!(wire.stdout.is_empty(), "{}: unexpected advice", row["id"]);
            for field in ["reason", "alternative"] {
                if let Some(text) = row[field].as_str() {
                    assert!(
                        wire.stderr.contains(text),
                        "{consumer} {}: missing {field}: {}",
                        row["id"],
                        wire.stderr
                    );
                }
            }
            if let Some(text) = row["reason_absent"].as_str() {
                assert!(
                    !wire.stderr.contains(text),
                    "{}: inaccurate location: {}",
                    row["id"],
                    wire.stderr
                );
            }
            if wire.exit == 0 {
                assert!(wire.stderr.is_empty(), "{}", row["id"]);
            }
            if row["native"] == true {
                use std::{
                    io::Write,
                    process::{Command, Stdio},
                };
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
                    Some(wire.exit),
                    "native {}",
                    row["id"]
                );
                assert_eq!(
                    output.stdout,
                    wire.stdout.as_bytes(),
                    "native {}",
                    row["id"]
                );
                assert_eq!(
                    output.stderr,
                    wire.stderr.as_bytes(),
                    "native {}",
                    row["id"]
                );
            }
        }
    }
    assert!(consumed > 0, "empty behavior partition {name}");
    println!("{name}: {consumed} rows, {evaluated} consumer evaluations");
}

#[test]
fn wildcard_basenames_intersect_sensitive_patterns() {
    partition("wildcard");
}

#[test]
fn macos_aliases_keep_lexical_protection_before_probes() {
    partition("aliases");
}

#[test]
fn grep_tool_preserves_relative_glob_segments() {
    partition("grep-glob");
}

#[test]
fn codex_native_shell_envelopes_use_the_tool_decoder() {
    partition("codex-native");
}

#[test]
fn descriptor_operands_check_redirects_without_probing_inherited_fds() {
    partition("descriptors");
}

#[test]
fn credential_resource_changes_are_distinct_from_metadata_maintenance() {
    partition("resource-change");
}

#[test]
fn recursive_search_on_named_public_files_does_not_traverse_hidden_files() {
    partition("recursive-file");
}

#[test]
fn ssh_scope_reasons_describe_the_named_directory() {
    partition("ssh-scope");
}

#[test]
fn cost_identity_and_deadline_refusals_give_concrete_next_steps() {
    partition("refusal-advice");
}
