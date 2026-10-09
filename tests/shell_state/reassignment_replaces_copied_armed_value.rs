use crate::program_contract as batch6;

#[test]
fn reassignment_replaces_copied_armed_value() {
    batch6::partition(
        "overwrite",
        include_str!("../fixtures/rust-batch6-overwrite.json"),
    );
}
