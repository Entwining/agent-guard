mod support;

use agent_guard_rust::{Event, adapters, evaluate_with_arm, shell};
use serde_json::{Value, json};

#[test]
fn wide_identifier_headers_and_logical_chains_keep_budget_recovery() {
    let fixture = support::Fixture::new();
    let packet: Value =
        serde_json::from_str(include_str!("fixtures/rust-shell-chain-cost.json")).unwrap();
    let header = (0..packet["header_words"].as_u64().unwrap())
        .map(|n| format!("public{n}"))
        .collect::<Vec<_>>()
        .join(" ");
    let chain = std::iter::repeat_n("true", packet["chain_terms"].as_u64().unwrap() as usize)
        .collect::<Vec<_>>()
        .join(" && ");
    for row in packet["rows"].as_array().unwrap() {
        let source = row["command"]
            .as_str()
            .unwrap()
            .replace("$HEADER", &header)
            .replace("$CHAIN", &chain);
        let body = fixture.body(&json!({"tool":"Bash","input":{"command":source}}));
        for consumer in ["claude", "codex", "pi"] {
            let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate_with_arm(
                Event {
                    bytes: &body,
                    context: &context,
                    probe: &mut probe,
                },
                shell::Arm::Brush,
            );
            assert_eq!(
                support::class(&result),
                "UR",
                "{consumer}: {}: {result:?}",
                row["id"]
            );
            let output = adapters::render(context.consumer, &result);
            assert_eq!(
                output.exit,
                row["exit"].as_i64().unwrap() as i32,
                "{consumer}: {}: {}",
                row["id"],
                output.stderr
            );
            assert!(output.stdout.is_empty(), "unexpected advice: {}", row["id"]);
            let stderr = output.stderr;
            for field in ["reason", "alternative"] {
                assert!(
                    stderr.contains(row[field].as_str().unwrap()),
                    "{consumer}: {}: {stderr}",
                    row["id"]
                );
            }
        }
    }
}
