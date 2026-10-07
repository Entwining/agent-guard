pub(crate) use crate::support;
pub use agent_guard_rust::{Event, adapters, evaluate_with_arm, shell::Arm};
pub use serde_json::{Value, json};

pub fn partition(name: &str) {
    let fixture = support::Fixture::new();
    let packet: Value =
        serde_json::from_str(include_str!("../../fixtures/rust-batch3.json")).unwrap();
    let rows: Vec<_> = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["partition"] == name)
        .collect();
    assert!(!rows.is_empty(), "missing credential role partition {name}");
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
            assert_eq!(wire.exit, if row["expected"] == "D" { 2 } else { 0 });
            assert!(wire.stdout.is_empty());
            if row["expected"] == "D" {
                assert!(!wire.stderr.is_empty());
                if let Some(reason) = row["reason_contains"].as_str() {
                    let agent_guard_rust::Outcome::ProtectedDenial { reason: denial, .. } =
                        &result.as_ref().unwrap().outcome
                    else {
                        panic!("missing denial")
                    };
                    assert!(
                        denial.effect.contains(reason),
                        "{}: {}",
                        row["id"],
                        wire.stderr
                    );
                }
                assert!(!result.as_ref().unwrap().effects.is_empty());
                if row["write"] == true {
                    let agent_guard_rust::Outcome::ProtectedDenial { reason, .. } =
                        &result.as_ref().unwrap().outcome
                    else {
                        panic!("missing write denial")
                    };
                    if row["reason_contains"]
                        .as_str()
                        .is_some_and(|text| text.contains("App Data"))
                    {
                        assert_eq!(reason.rule, agent_guard_rust::DenialRule::AppData);
                        assert_eq!(
                            reason.rule.message(),
                            "This reads a protected macOS app-data directory. Name a specific non-sensitive file under ~/Library/Application Support instead, or ask the user to inspect the protected file and share the needed fact."
                        );
                    }
                    assert!(
                        result
                            .as_ref()
                            .unwrap()
                            .effects
                            .iter()
                            .any(|effect| matches!(
                                effect,
                                agent_guard_rust::EffectRecord::ProtectedTarget { write: true, .. }
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
