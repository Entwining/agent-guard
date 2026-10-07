use crate::support;
use agent_guard_rust::{
    Event, evaluate_with_arm,
    shell::{self, Arm},
};
use serde_json::{Value, json};

#[test]
fn quoted_tilde_runtime_prefix_stays_relative() {
    let fixture = support::Fixture::new();
    let packet: Value =
        serde_json::from_str(include_str!("../fixtures/rust-batch1-b1b.json")).unwrap();
    let rows = packet["rows"].as_array().unwrap();
    assert!(!rows.is_empty(), "missing quoted-tilde runtime partition");
    for row in rows {
        for consumer in ["claude", "codex", "pi"] {
            let context = fixture.context(&json!({"consumer":consumer,"cwd":row["cwd"]}));
            let source = row["source"].as_str().unwrap();
            let bytes = serde_json::to_vec(&json!({"tool_name":if consumer=="pi" {"bash"} else {"Bash"},"tool_input":{"command":source}})).unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate_with_arm(
                Event {
                    bytes: &bytes,
                    context: &context,
                    probe: &mut probe,
                },
                Arm::Brush,
            );
            let class = support::class(&result);
            let good = if row["expected"] == "allow" {
                ["N", "UC"].contains(&class)
            } else {
                class == "D"
            };
            assert!(
                good,
                "{}: {consumer}: {result:?}; records={:?}",
                row["id"],
                shell::observe(
                    source,
                    Arm::Brush,
                    &context.home,
                    &context.cwd,
                    context.zsh_executor
                )
                .unwrap()
                .script
                .commands
            );
            let wire = agent_guard_rust::adapters::render(context.consumer, &result);
            assert_eq!(wire.exit, if row["expected"] == "allow" { 0 } else { 2 });
            assert!(wire.stdout.is_empty());
        }
    }
}
