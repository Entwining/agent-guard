mod support;
use agent_guard_rust::{Event, adapters, evaluate_with_arm, shell::Arm};
use serde_json::{Value, json};

fn partition(name: &str) {
    let fixture = support::Fixture::new();
    let packet: Value = serde_json::from_str(include_str!("fixtures/rust-batch3b.json")).unwrap();
    let rows: Vec<_> = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["partition"] == name)
        .collect();
    assert!(!rows.is_empty());
    for row in rows {
        for consumer in ["claude", "codex", "pi"] {
            let context = fixture.context(&json!({"consumer":consumer,"cwd":row["cwd"]}));
            let bytes = serde_json::to_vec(&json!({"tool_name":if consumer=="pi" {"bash"} else {"Bash"},"tool_input":{"command":fixture.expand(row["source"].as_str().unwrap())}})).unwrap();
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
            assert_eq!(wire.exit, if row["expected"] == "D" { 2 } else { 0 });
            assert!(wire.stdout.is_empty());
            if row["expected"] == "D" {
                assert!(!wire.stderr.is_empty());
                assert!(!result.as_ref().unwrap().effects.is_empty());
                if row["id"] == "b3b-x-cd1-git" {
                    assert!(
                        result
                            .as_ref()
                            .unwrap()
                            .effects
                            .iter()
                            .any(|effect| matches!(
                                effect,
                                agent_guard_rust::EffectRecord::ProtectedTarget {
                                    protection: agent_guard_rust::filesystem::Protection::AppData,
                                    source: agent_guard_rust::EffectSource::Cwd,
                                    write: false,
                                }
                            ))
                    );
                }
            } else if row["expected"] == "N" {
                assert!(wire.stderr.is_empty());
            }
            assert!(!matches!(
                result.as_ref().unwrap().outcome,
                agent_guard_rust::Outcome::SoftAdvice(_)
            ));
        }
    }
}

#[test]
fn tracked_cwd_entry_follows_go_named_target_rule() {
    partition("cwd");
}
