use crate::program_contract as contract;

const ROWS: &str = include_str!("../fixtures/rust-denial-triggers.json");

#[test]
fn path_denials_name_the_path() {
    contract::partition("path", ROWS);
}

#[test]
fn broad_denials_name_the_scan_root() {
    contract::partition("broad", ROWS);
}

#[test]
fn prioritized_denials_name_their_own_trigger() {
    contract::partition("priority", ROWS);
}

#[test]
fn command_denials_name_the_program() {
    contract::partition("program", ROWS);
}
