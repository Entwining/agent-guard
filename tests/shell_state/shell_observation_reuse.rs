#[test]
fn reused_shell_syntax_preserves_context_protection_and_failure() {
    crate::program_contract::partition_with(
        "reuse",
        include_str!("../fixtures/rust-batch17e-shell-reuse.json"),
        |row, evaluation| {
            if row["expected"] == "N" || row["expected"] == "UC" {
                assert!(!matches!(
                    evaluation.outcome,
                    agent_guard_rust::Outcome::SoftAdvice(_)
                ));
                if let agent_guard_rust::Coverage::LimitedPreflight(gaps) = &evaluation.coverage {
                    assert!(!gaps.contains(&agent_guard_rust::CoverageGap::InspectionBudget));
                }
            }
        },
    );
}
