use agent_guard_rust::{CoverageGap, Outcome};

#[test]
fn brace_alternatives_past_the_bound_refuse() {
    crate::program_contract::partition_with(
        "brace-bound",
        include_str!("../fixtures/rust-brace-alternative-bound.json"),
        |row, evaluation| {
            if row["expected"] == "UR" {
                assert!(
                    matches!(
                        &evaluation.outcome,
                        Outcome::CoverageInsufficient {
                            cause: CoverageGap::InspectionBudget,
                            ..
                        }
                    ),
                    "{}: {:?}",
                    row["id"],
                    evaluation.outcome
                );
            }
        },
    );
}
