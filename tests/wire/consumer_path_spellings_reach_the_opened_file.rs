use crate::support;
use agent_guard_rust::{Event, adapters, evaluate_with_arm, shell::Arm};
use serde_json::{Value, json};

#[test]
fn consumer_path_spellings_reach_the_opened_file() {
    let fixture = support::Fixture::new();
    let packet: Value = serde_json::from_str(include_str!(
        "../fixtures/rust-consumer-path-normalization.json"
    ))
    .unwrap();
    let mut mismatches = Vec::new();
    for row in packet["rows"].as_array().unwrap() {
        fixture.setup(row);
        let context = fixture.context(&json!({"consumer":row["consumer"],"cwd":"$P"}));
        let bytes = serde_json::to_vec(
            &json!({"tool_name":row["tool"],"tool_input":fixture.expand_value(&row["input"])}),
        )
        .unwrap();
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        let result = evaluate_with_arm(
            Event {
                bytes: &bytes,
                context: &context,
                probe: &mut probe,
            },
            Arm::Brush,
        );
        if support::class(&result) != row["expected"] {
            mismatches.push(format!("{}: {result:?}", row["id"]));
            continue;
        }
        let wire = adapters::render(context.consumer, &result);
        let refused = row["expected"] != "N";
        assert_eq!(wire.exit, if refused { 2 } else { 0 }, "{}", row["id"]);
        assert_eq!(wire.stderr.is_empty(), !refused, "{}", row["id"]);
    }
    assert!(mismatches.is_empty(), "{mismatches:#?}");
}
