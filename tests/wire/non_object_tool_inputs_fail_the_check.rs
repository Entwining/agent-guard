use crate::program_contract as batch6;

#[test]
fn non_object_tool_inputs_fail_the_check() {
    batch6::partition(
        "input-shape",
        include_str!("../fixtures/rust-tool-input-shape.json"),
    );
}
