pub(crate) use crate::support;
pub use agent_guard_rust::{
    Event, evaluate_with_arm,
    filesystem::{self, Probe},
    record::{Direction, Effect, HostFacts, Target, Via, Walk},
    shell::{self, Arm},
};
pub use serde_json::{Value, json};
pub use std::{panic::AssertUnwindSafe, path::Path};

pub fn regressions(owner: &str) {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("../../fixtures/rust-m1-1-regressions.json")).unwrap();
    let fixture = support::Fixture::new();
    for consumer in ["claude", "codex", "pi"] {
        let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
        let selected: Vec<_> = rows.iter().filter(|r| r["owner"] == owner).collect();
        assert!(!selected.is_empty(), "missing owner {owner}");
        for row in selected {
            let body = serde_json::to_vec(&json!({
                "tool_name":if consumer == "pi" {"bash"} else {"Bash"},
                "tool_input":{"command":fixture.expand(row["source"].as_str().unwrap())}
            }))
            .unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate_with_arm(
                Event {
                    bytes: &body,
                    context: &context,
                    probe: &mut probe,
                },
                Arm::Brush,
            );
            assert_eq!(
                support::class(&result),
                row["expected"],
                "{consumer}: {row}"
            );
            let wire = agent_guard_rust::adapters::render(context.consumer, &result);
            assert_eq!(
                wire.exit,
                if row["expected"] == "D" || row["expected"] == "UR" {
                    2
                } else {
                    0
                }
            );
            assert!(wire.stdout.is_empty(), "unexpected advice: {row}");
            assert_eq!(
                wire.stderr.is_empty(),
                row["expected"] == "N" || row["expected"] == "UC"
            );
            if let agent_guard_rust::Outcome::ProtectedDenial { reason, .. } =
                &result.as_ref().unwrap().outcome
            {
                assert!(wire.stderr.contains(reason.rule.message()));
            }
        }
    }
}
