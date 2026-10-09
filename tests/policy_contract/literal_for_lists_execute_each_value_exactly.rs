#[path = "../support/cases/literal_for_lists_execute_each_value_exactly.rs"]
pub mod cases;
use cases::*;

#[test]
fn rg_include_advice_follows_go() {
    contract_rows(&["search[50]", "search[51]"]);
    let fixture = support::Fixture::new();
    for consumer in ["claude", "codex", "pi"] {
        let context = fixture.context(&json!({"consumer":consumer, "cwd":"$P"}));
        let bytes = br#"{"tool_name":"Bash","tool_input":{"command":"rg --include '*.rs' needle public; rg --include '*.rs' needle public; rg 'a\\|b' public"}}"#;
        let result = evaluate_with_arm(
            Event {
                bytes,
                context: &context,
                probe: &mut support::RecordingProbe::literal_for_quoted_paths(&fixture),
            },
            Arm::Brush,
        );
        let agent_guard_rust::Outcome::SoftAdvice(advice) = &result.as_ref().unwrap().outcome
        else {
            panic!("missing advice: {result:?}")
        };
        assert_eq!(advice.len(), 2);
        assert!(advice.contains(&agent_guard_rust::Advice::RgInclude));
        assert!(advice.contains(&agent_guard_rust::Advice::RgBre));
        let wire = adapters::render(context.consumer, &result);
        assert_eq!(wire.exit, 0);
        assert!(wire.stderr.is_empty());
        if consumer == "claude" {
            let rendered: Value = serde_json::from_str(&wire.stdout).unwrap();
            let text = rendered["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap();
            for message in advice.iter().map(agent_guard_rust::Advice::message) {
                assert_eq!(text.matches(message).count(), 1);
            }
        } else {
            assert!(wire.stdout.is_empty());
        }
    }
}

#[test]
fn rg_bre_advice_follows_go() {
    contract_rows(&[
        "search[52]",
        "search[53]",
        "search[54]",
        "search[55]",
        "search[62]",
    ]);
}

#[test]
fn secret_reasons_follow_their_go_owner() {
    contract_rows(&["credentials[128]", "readers[86]"]);
}

#[test]
fn target_advice_and_probe_controls_preserve_their_contract() {
    partition("item5-controls");
}
