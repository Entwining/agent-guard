use crate::program_contract as contract;

#[test]
fn unconditional_appends_preserve_complete_values() {
    contract::partition(
        "exact",
        include_str!("../fixtures/rust-batch17c-fields.json"),
    );
}
