use crate::program_contract as contract;

#[test]
fn unconditional_appends_preserve_complete_values() {
    contract::partition(
        "exact",
        include_str!("../fixtures/rust-batch17c-fields.json"),
    );
}

#[test]
fn producer_candidates_reach_xargs_operands() {
    contract::partition(
        "pipeline",
        include_str!("../fixtures/rust-batch17c-fields.json"),
    );
}
