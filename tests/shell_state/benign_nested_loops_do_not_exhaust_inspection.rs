use crate::program_contract as batch9;

#[test]
fn benign_nested_loops_do_not_exhaust_inspection() {
    batch9::partition("benign", include_str!("../fixtures/rust-batch9-loops.json"));
}

#[test]
fn nested_loops_keep_protected_and_directory_controls() {
    batch9::partition(
        "control",
        include_str!("../fixtures/rust-batch9-loops.json"),
    );
}
