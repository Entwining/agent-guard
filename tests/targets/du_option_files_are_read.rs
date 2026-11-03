use crate::program_contract as batch6;

#[test]
fn du_option_files_are_read() {
    batch6::partition("du", include_str!("../fixtures/rust-batch6.json"));
}
