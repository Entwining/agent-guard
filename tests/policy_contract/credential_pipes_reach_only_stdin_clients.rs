const ROWS: &str = include_str!("../fixtures/rust-credential-stdin.json");

#[test]
fn credentials_piped_to_a_client_standard_input_are_consumed() {
    crate::program_contract::partition("consumed", ROWS);
}

#[test]
fn credentials_piped_anywhere_else_are_printed() {
    crate::program_contract::partition("printed", ROWS);
}
