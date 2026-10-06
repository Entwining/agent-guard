#[path = "support/batch6.rs"]
mod contract;

#[test]
fn unknown_read_aggregation_converges_without_literal_path_loss() {
    contract::partition("control", include_str!("fixtures/rust-batch10-loops.json"));
}

#[test]
fn aggregation_preserves_protected_sources_and_literal_candidates() {
    contract::partition(
        "protected",
        include_str!("fixtures/rust-batch10-loops.json"),
    );
}
