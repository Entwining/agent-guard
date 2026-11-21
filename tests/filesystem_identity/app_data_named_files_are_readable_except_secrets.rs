use crate::program_contract as contract;

const ROWS: &str = include_str!("../fixtures/rust-app-data-named-files.json");

#[test]
fn single_file_readers_may_read_one_named_container_file() {
    contract::partition("reader", ROWS);
}

#[test]
fn programs_that_list_or_recurse_keep_the_container_refusal() {
    contract::partition("walk", ROWS);
}

#[test]
fn secrets_inside_a_container_keep_their_own_protection() {
    contract::partition("secret", ROWS);
}
