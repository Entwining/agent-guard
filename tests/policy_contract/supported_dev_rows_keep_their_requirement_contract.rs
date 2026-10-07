use crate::support;

#[test]
fn supported_dev_rows_keep_their_requirement_contract() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for row in support::rows()
        .iter()
        .filter(|r| support::is_evaluator_row(r))
    {
        for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
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
fn declared_metamorphic_variants_preserve_the_contract() {
    let rows = support::rows();
    for variant in rows
        .iter()
        .filter(|r| r.get("metamorphic_variant").is_some())
    {
        let base = rows
            .iter()
            .find(|r| r["id"] == variant["metamorphic_variant"]["base_row_id"])
            .unwrap();
        for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
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
