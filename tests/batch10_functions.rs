#[path = "support/batch6.rs"]
mod contract;

#[test]
fn and_success_continuation_defines_public_helpers() {
    contract::partition(
        "control",
        include_str!("fixtures/rust-batch10-functions.json"),
    );
}

#[test]
fn conditional_helpers_keep_protected_body_and_enclosing_branch_checks() {
    contract::partition(
        "protected",
        include_str!("fixtures/rust-batch10-functions.json"),
    );
}
