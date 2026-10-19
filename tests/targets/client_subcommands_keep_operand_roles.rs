const ROWS: &str = include_str!("../fixtures/rust-client-subcommands.json");

#[test]
fn cargo_subcommands_keep_operand_roles() {
    crate::program_contract::partition("cargo", ROWS);
}

#[test]
fn produced_items_take_the_client_operand_role() {
    crate::program_contract::partition("items", ROWS);
}

#[test]
fn query_verbs_name_volumes_and_records() {
    crate::program_contract::partition("queries", ROWS);
}
