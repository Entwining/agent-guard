#[path = "support/differential.rs"]
mod differential;
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
            probe.fault = row["fault"].as_str().map(|path| fixture.expand(path));
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
                if row["expected"] == "D" || row["expected"] == "UR" || row["expected"] == "F" {
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

#[test]
fn unset_removes_only_executed_named_bindings() {
    partition("unset");
}

#[test]
fn git_environment_uses_git_directory_roles() {
    partition("git-environment");
}

fn contract_rows(ids: &[&str]) {
    for row in differential::selected_report(Arm::Brush, ids) {
        for observation in row["observations"].as_array().unwrap() {
            assert_ne!(
                observation["category"], "Rust_defect",
                "{}: {observation}",
                row["id"]
            );
            assert_eq!(
                observation["exit"],
                if observation["expected"] == "D" { 2 } else { 0 },
                "{}: {observation}",
                row["id"]
            );
            assert_eq!(
                observation["stdout"].as_str().unwrap().is_empty(),
                observation["expected"] != "A",
                "{}: {observation}",
                row["id"]
            );
            assert_eq!(
                observation["stderr"].as_str().unwrap().is_empty(),
                observation["expected"] != "D",
                "{}: {observation}",
                row["id"]
            );
        }
    }
}

#[test]
fn bundle_output_is_a_write() {
    contract_rows(&["programs[42]"]);
}
#[test]
fn tar_exclude_is_a_pattern_name() {
    contract_rows(&["programs[64]"]);
}
#[test]
fn rm_operand_is_metadata() {
    contract_rows(&["shell[0]"]);
}
#[test]
fn long_commit_message_is_not_a_probe_fault() {
    contract_rows(&["shell[175]"]);
}
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
