use crate::program_contract as contract;

#[test]
fn conditional_literal_accumulation_allows_public_output() {
    contract::partition(
        "public",
        include_str!("../fixtures/rust-batch17b-accumulation.json"),
    );
}

#[test]
fn conditional_literal_accumulation_preserves_read_targets() {
    contract::partition(
        "protected",
        include_str!("../fixtures/rust-batch17b-accumulation.json"),
    );
}

#[test]
fn repeated_copies_keep_distinct_resource_prefixes() {
    contract::partition(
        "identity",
        include_str!("../fixtures/rust-batch17b-accumulation.json"),
    );
}

#[test]
fn repeated_public_output_keeps_substitution_observation() {
    contract::partition(
        "output",
        include_str!("../fixtures/rust-batch17b-accumulation.json"),
    );
}
