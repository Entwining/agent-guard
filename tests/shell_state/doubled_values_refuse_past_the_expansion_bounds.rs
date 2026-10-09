use agent_guard_rust::{CoverageGap, Outcome};

#[test]
fn doubled_values_refuse_past_the_expansion_bounds() {
    crate::program_contract::partition_with(
        "value-bounds",
        include_str!("../fixtures/rust-expansion-value-bounds.json"),
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
