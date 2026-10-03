mod support;
use agent_guard_rust::shell::Arm;

#[test]
fn lifecycle_harness_only() {
    let rows = support::rows();
    let lifecycle: Vec<_> = rows
        .iter()
        .filter(|row| support::is_lifecycle_row(row))
        .collect();
    assert_eq!(lifecycle.len(), 9);
    for row in lifecycle {
        assert!(!support::is_evaluator_row(row));
        let result = support::run(row, Arm::StructuredOnly);
        support::assert_tuple(row, &result);
        assert_eq!(result["evidence_owner"], "harness-only");
        assert_eq!(result["shell_observation_entries"], 0);
        assert_eq!(result["lifecycle"]["ready_receipt"], true);
        assert_eq!(result["lifecycle"]["reaped"], true);
        assert_eq!(result["lifecycle"]["completion_observed_by_wait"], true);
        println!("{result}");
    }
}

#[test]
fn dev_contract() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for row in support::rows()
        .iter()
        .filter(|r| support::is_evaluator_row(r))
    {
        for arm in [Arm::Brush, Arm::TreeSitter] {
            let result = support::run(row, arm);
            checked += 1;
            if std::panic::catch_unwind(|| support::assert_tuple(row, &result)).is_err() {
                failures.push(format!("{} {:?}: {}", row["id"], arm, result["library"]));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures in {checked} checks:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn metamorphic_variants() {
    let rows = support::rows();
    for variant in rows
        .iter()
        .filter(|r| r.get("metamorphic_variant").is_some())
    {
        let base = rows
            .iter()
            .find(|r| r["id"] == variant["metamorphic_variant"]["base_row_id"])
            .unwrap();
        for arm in [Arm::Brush, Arm::TreeSitter] {
            let base_result = support::run(base, arm);
            let variant_result = support::run(variant, arm);
            for field in [
                "class",
                "coverage",
                "exit",
                "stdout",
                "operation_start_count",
            ] {
                assert_eq!(
                    base_result[field], variant_result[field],
                    "{} metamorphic {field}",
                    variant["id"]
                );
            }
            if base_result["class"] == "UR" {
                assert_eq!(
                    base_result["recovery_receipt"]["task_result"],
                    variant_result["recovery_receipt"]["task_result"]
                );
            } else {
                assert_eq!(
                    base_result["effects"]["protected_access_count"],
                    variant_result["effects"]["protected_access_count"]
                );
            }
        }
    }
}
