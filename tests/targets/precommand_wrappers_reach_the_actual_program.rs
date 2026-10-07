use crate::support;
use agent_guard_rust::{Event, adapters, evaluate_with_arm, shell::Arm};
use serde_json::{Value, json};

fn partition(name: &str) {
    let fixture = support::Fixture::new();
    let packet: Value = serde_json::from_str(include_str!("../fixtures/rust-batch2.json")).unwrap();
    let rows: Vec<_> = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["partition"] == name)
        .collect();
    assert!(!rows.is_empty(), "missing wrapper role partition {name}");
    for row in rows {
        for consumer in ["claude", "codex", "pi"] {
            let context = fixture.context(&json!({"consumer":consumer,"cwd":row["cwd"]}));
            let bytes = serde_json::to_vec(&json!({"tool_name":if consumer=="pi" {"bash"} else {"Bash"},"tool_input":{"command":row["source"]}})).unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate_with_arm(
                Event {
                    bytes: &bytes,
                    context: &context,
                    probe: &mut probe,
                },
                Arm::Brush,
            );
            assert_eq!(
                support::class(&result),
                row["expected"],
                "{consumer}: {}: {result:?}",
                row["id"]
            );
            let wire = adapters::render(context.consumer, &result);
            let allowed = matches!(row["expected"].as_str().unwrap(), "N" | "UC");
            assert_eq!(wire.exit, if allowed { 0 } else { 2 });
            assert!(wire.stdout.is_empty());
            if let Some(effect) = row["effect"].as_str() {
                use agent_guard_rust::{EffectRecord, filesystem::Protection};
                let effects = &result.as_ref().unwrap().effects;
                assert!(
                    effects.iter().any(|v| matches!(
                        (effect, v),
                        (
                            "AppData",
                            EffectRecord::ProtectedTarget {
                                protection: Protection::AppData,
                                ..
                            },
                        ) | (
                            "CredentialFile",
                            EffectRecord::ProtectedTarget {
                                protection: Protection::Environment | Protection::Credential,
                                ..
                            },
                        ) | ("EnvironmentDump", EffectRecord::EnvironmentDump)
                            | ("CredentialVariable", EffectRecord::CredentialVariable)
                            | ("HiddenContent", EffectRecord::HiddenContent)
                    )),
                    "{}: {result:?}",
                    row["id"]
                );
                assert!(!wire.stderr.is_empty());
                if row["go_expected"][consumer]["reason"]
                    .as_str()
                    .is_some_and(|reason| reason.starts_with("This inline code"))
                {
                    assert!(
                        wire.stderr.contains("inline code"),
                        "{}: {}",
                        row["id"],
                        wire.stderr
                    );
                    assert!(effects.iter().any(|effect| matches!(
                        effect,
                        EffectRecord::ProtectedTarget {
                            source: agent_guard_rust::EffectSource::InlineCode,
                            ..
                        }
                    )));
                }
            }
            assert!(
                !matches!(
                    result.as_ref().unwrap().outcome,
                    agent_guard_rust::Outcome::SoftAdvice(_)
                ),
                "{}: {result:?}",
                row["id"]
            );
            if row["expected"] == "N" {
                assert!(wire.stderr.is_empty());
            }
        }
    }
}

#[test]
fn precommand_wrappers_reach_the_actual_program() {
    partition("wrappers");
}

#[test]
fn producers_feed_decoded_shell_stdin() {
    partition("producer");
}

#[test]
fn xargs_consumes_produced_items_and_here_data() {
    partition("xargs");
}

#[test]
fn interpreter_stdin_and_display_body_variables_are_inspected() {
    partition("stdin");
}

#[test]
fn git_file_options_keep_read_roles() {
    partition("git");
}
