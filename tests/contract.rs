mod support;
use agent_guard_rust::shell::Arm;

#[test]
fn dev_contract() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for row in support::rows()
        .iter()
        .filter(|r| r["consumer"] != "owned-writer")
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
