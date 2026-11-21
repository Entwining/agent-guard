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

#[test]
fn unknown_programs_keep_independent_shell_effects() {
    crate::program_contract::partition("shell-effects", ROWS);
}

#[test]
fn gh_search_terms_endpoints_and_literal_fields_are_not_paths() {
    crate::program_contract::partition("gh-names", ROWS);
}

#[test]
fn gh_api_file_inputs_keep_read_roles() {
    crate::program_contract::partition("gh-reads", ROWS);
}

#[test]
fn mkcert_writes_outputs_reads_its_csr_and_names_hosts() {
    crate::program_contract::partition("mkcert", ROWS);
}
