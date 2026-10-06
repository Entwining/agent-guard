#[path = "support/batch6.rs"]
mod contract;

#[test]
fn repeated_loop_references_share_one_binding_candidate() {
    contract::partition(
        "correlation",
        include_str!("fixtures/rust-batch10c-loops.json"),
    );
}

#[test]
fn completed_literal_loop_does_not_make_later_function_conditional() {
    contract::partition("scope", include_str!("fixtures/rust-batch10c-loops.json"));
}
