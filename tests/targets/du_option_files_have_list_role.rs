use crate::program_contract as batch6;

#[test]
fn du_option_files_have_list_role() {
    batch6::partition("du", include_str!("../fixtures/rust-batch6.json"));
}
