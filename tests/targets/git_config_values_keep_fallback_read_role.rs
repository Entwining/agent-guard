#[test]
fn git_config_values_keep_fallback_read_role() {
    crate::program_contract::partition(
        "config",
        include_str!("../fixtures/rust-git-config-values.json"),
    );
}
