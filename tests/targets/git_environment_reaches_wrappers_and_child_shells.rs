use crate::program_contract as batch6;

#[test]
fn git_environment_reaches_wrappers_and_child_shells() {
    batch6::partition("git-env", include_str!("../fixtures/rust-batch6.json"));
}
