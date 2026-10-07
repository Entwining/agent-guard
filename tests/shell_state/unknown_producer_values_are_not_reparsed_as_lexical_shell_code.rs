use crate::program_contract as contract;

#[test]
fn unknown_producer_values_are_not_reparsed_as_lexical_shell_code() {
    contract::partition(
        "unknown",
        include_str!("../fixtures/rust-batch10c-pipeline-values.json"),
    );
}

#[test]
fn known_shell_producers_keep_nested_protection() {
    contract::partition(
        "known",
        include_str!("../fixtures/rust-batch10c-pipeline-values.json"),
    );
}
