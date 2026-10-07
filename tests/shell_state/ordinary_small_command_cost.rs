#[test]
fn unknown_repetition_preserves_fixed_path_candidates() {
    for partition in ["aggregate", "aggregate-protected"] {
        crate::program_contract::partition_with(
            partition,
            include_str!("../fixtures/rust-batch17-cost.json"),
            |row, result| {
                if row["id"] == "unknown-parent-repetition" {
                    assert!(
                        matches!(&result.coverage, agent_guard_rust::Coverage::LimitedPreflight(gaps)
                        if gaps.contains(&agent_guard_rust::CoverageGap::UnsupportedShellSyntax)),
                        "parent normalization must remain an unmodelled state: {:?}",
                        result.coverage
                    );
                }
            },
        );
    }
}

#[test]
fn long_patterns_preserve_protected_targets() {
    for partition in [
        "pattern",
        "pattern-protected",
        "matching",
        "matching-protected",
    ] {
        crate::program_contract::partition(
            partition,
            include_str!("../fixtures/rust-batch17-cost.json"),
        );
    }
}

#[test]
fn substitution_candidate_union_preserves_protected_reads() {
    crate::program_contract::partition(
        "substitution",
        include_str!("../fixtures/rust-batch17-cost.json"),
    );
}
