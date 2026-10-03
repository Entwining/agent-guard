mod support;
use agent_guard_rust::shell::Arm;
use serde_json::json;

#[test]
fn reason_and_effect_partitions_reject_missing_evidence() {
    let rows = support::rows();
    for arm in [Arm::Brush, Arm::TreeSitter] {
        for id in [
            "S01-read-appdata-claude",
            "S04-shell-home-explicit-claude",
            "S14-qualifier-protected-claude",
        ] {
            let row = rows.iter().find(|row| row["id"] == id).unwrap();
            let actual = support::run(row, arm);
            support::assert_preflight_tuple(row, &actual);
            let mut missing = actual.clone();
            missing["observed_effects"] = json!([]);
            assert!(
                std::panic::catch_unwind(|| support::assert_preflight_tuple(row, &missing))
                    .is_err(),
                "{id} missing effect must fail"
            );
        }
        let row = rows
            .iter()
            .find(|row| {
                row["reason_contract"]["effect_or_failure"]
                    == "broad recursive root reaches protected locations"
            })
            .unwrap();
        let mut actual = support::run(row, arm);
        actual["stderr"] = json!(
            "broad recursive root reaches protected locations; recovery: recheck protected scope"
        );
        assert!(
            std::panic::catch_unwind(|| support::assert_preflight_tuple(row, &actual)).is_err()
        );
    }
}

#[test]
fn outside_tool_class_rejects_a_different_tool() {
    let rows = support::rows();
    for row in rows
        .iter()
        .filter(|row| row["expected_coverage"].get("tool_class").is_some())
    {
        let mut actual = support::run(row, Arm::Brush);
        support::assert_preflight_tuple(row, &actual);
        actual["coverage"]["tool_class"] = json!("different-tool");
        assert!(
            std::panic::catch_unwind(|| support::assert_preflight_tuple(row, &actual)).is_err()
        );
    }
}
