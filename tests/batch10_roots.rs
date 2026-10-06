#[path = "support/batch6.rs"]
mod contract;

#[test]
fn opaque_operands_refuse_broad_roots() {
    contract::partition(
        "protected",
        include_str!("fixtures/rust-batch10-roots.json"),
    );
}

#[test]
fn nonpath_and_metadata_operands_preserve_public_controls() {
    contract::partition("control", include_str!("fixtures/rust-batch10-roots.json"));
}
