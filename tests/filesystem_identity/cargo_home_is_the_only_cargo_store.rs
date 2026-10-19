const ROWS: &str = include_str!("../fixtures/rust-cargo-store.json");

#[test]
fn project_cargo_configuration_is_public() {
    crate::program_contract::partition("project-cargo", ROWS);
}

#[test]
fn cargo_credential_names_stay_protected_in_projects() {
    crate::program_contract::partition("project-cargo-file-name", ROWS);
}

#[test]
fn other_store_directories_are_stores_anywhere() {
    crate::program_contract::partition("store-outside-home", ROWS);
}
