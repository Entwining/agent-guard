#[test]
fn executable_inputs_preserve_stage_order_and_known_candidates() {
    crate::program_contract::partition_with(
        "executable-flow",
        include_str!("../fixtures/rust-executable-flow.json"),
        |row, evaluation| {
            if row["expected"] != "A" {
                assert!(!matches!(
                    evaluation.outcome,
                    agent_guard_rust::Outcome::SoftAdvice(_)
                ));
            }
        },
    );
}
