mod support;
use agent_guard_rust::{Event, adapters, evaluate_with_arm, shell::Arm};
use serde_json::{Value, json};

fn partition(name: &str) {
    let fixture = support::Fixture::new();
    let packet: Value = serde_json::from_str(include_str!("fixtures/rust-batch5.json")).unwrap();
    let rows: Vec<_> = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["partition"] == name)
        .collect();
    assert!(!rows.is_empty());
    for row in rows {
        for consumer in ["claude", "codex", "pi"] {
            let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
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
            assert_eq!(
                wire.exit,
                if row["expected"] == "D" || row["expected"] == "UR" {
                    2
                } else {
                    0
                }
            );
            assert!(wire.stdout.is_empty());
            assert_eq!(
                wire.stderr.is_empty(),
                row["expected"] == "N" || row["expected"] == "UC"
            );
        }
    }
}

#[test]
fn literal_for_lists_execute_each_value_exactly() {
    partition("literal-loop");
}
