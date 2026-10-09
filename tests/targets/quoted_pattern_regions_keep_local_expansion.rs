use crate::program_contract;

#[test]
fn quoted_pattern_regions_keep_local_expansion_and_roles() {
    program_contract::partition(
        "provenance",
        include_str!("../fixtures/rust-batch17e-patterns.json"),
    );
}
