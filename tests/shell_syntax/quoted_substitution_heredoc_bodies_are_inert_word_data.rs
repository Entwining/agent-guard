use crate::program_contract as contract;

#[test]
fn quoted_substitution_heredoc_bodies_are_inert_word_data() {
    contract::partition(
        "control",
        include_str!("../fixtures/rust-batch10-heredocs.json"),
    );
}

#[test]
fn unquoted_and_external_substitutions_keep_protected_effects() {
    contract::partition(
        "protected",
        include_str!("../fixtures/rust-batch10-heredocs.json"),
    );
}
