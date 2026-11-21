use crate::program_contract as contract;

const ROWS: &str = include_str!("../fixtures/rust-variable-dump-advice.json");

#[test]
fn variable_dumps_refuse_with_a_name_listing_alternative() {
    contract::partition("dump", ROWS);
}

#[test]
fn listing_variable_names_proceeds() {
    contract::partition("names", ROWS);
}
