use crate::program_contract as contract;

#[test]
fn uncertain_words_keep_grammar_positions() {
    contract::partition(
        "grammar-positions",
        include_str!("../fixtures/rust-uncertain-words-keep-grammar-positions.json"),
    );
}
