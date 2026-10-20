const ROWS: &str = include_str!("../fixtures/rust-literal-credential-paths.json");

#[test]
fn paths_that_spell_a_catalog_entry_are_credentials() {
    crate::program_contract::partition("catalog-path", ROWS);
}

#[test]
fn paths_below_a_catalog_match_are_credentials() {
    crate::program_contract::partition("below-catalog-match", ROWS);
}

#[test]
fn escaped_names_in_patterns_keep_their_identity() {
    crate::program_contract::partition("escaped-literal", ROWS);
}

#[test]
fn generic_names_under_other_directories_are_public() {
    crate::program_contract::partition("unspelled-directory", ROWS);
}
