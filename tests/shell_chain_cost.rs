mod support;

use agent_guard_rust::{CoverageGap, Event, Outcome, adapters, evaluate};
use serde_json::{Value, json};

#[test]
fn wide_identifier_headers_and_logical_chains_stop_at_the_budget() {
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
            let result = evaluate(Event {
                bytes: &body,
                context: &context,
                probe: &mut probe,
            });
            assert_eq!(
                support::class(&result),
                "UC",
                "{consumer}: {}: {result:?}",
                row["id"]
            );
            assert!(
                matches!(
                    &result.as_ref().unwrap().outcome,
                    Outcome::CoverageInsufficient {
                        cause: CoverageGap::InspectionBudget,
                        ..
                    }
                ),
                "{consumer}: {}: {result:?}",
                row["id"]
            );
            let output = adapters::render(context.consumer, &result);
            assert_eq!(
                (output.exit, output.stdout.as_str(), output.stderr.as_str()),
                (0, "", ""),
                "{consumer}: {}",
                row["id"]
            );
        }
    }
}
