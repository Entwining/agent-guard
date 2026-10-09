use crate::program_contract as batch7;

#[test]
fn wrapper_assignment_operands_are_unknown_programs() {
    batch7::partition(
        "control",
        include_str!("../fixtures/rust-batch7-wrappers.json"),
    );
}
