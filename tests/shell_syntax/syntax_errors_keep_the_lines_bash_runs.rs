use crate::program_contract as contract;

const ROWS: &str = include_str!("../fixtures/rust-syntax-error-prefix.json");

#[test]
fn lines_before_a_syntax_error_are_checked() {
    contract::partition("runnable-prefix", ROWS);
}

#[test]
fn lines_bash_does_not_reach_are_not_checked() {
    contract::partition("unrun-lines", ROWS);
}
