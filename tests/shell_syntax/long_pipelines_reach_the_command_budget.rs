use agent_guard_rust::{CoverageGap, Outcome};

#[test]
fn long_pipelines_reach_the_command_budget() {
    crate::program_contract::partition_with(
        "pipeline-stages",
        include_str!("../fixtures/rust-pipeline-stage-budget.json"),
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
