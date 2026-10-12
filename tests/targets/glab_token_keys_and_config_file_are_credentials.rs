#[test]
fn glab_config_get_prints_only_token_keys_as_tokens() {
    crate::program_contract::partition(
        "config-get",
        include_str!("../fixtures/rust-glab-credentials.json"),
    );
}

#[test]
fn glab_config_file_is_a_credential_and_its_aliases_are_not() {
    crate::program_contract::partition(
        "config-file",
        include_str!("../fixtures/rust-glab-credentials.json"),
    );
}
