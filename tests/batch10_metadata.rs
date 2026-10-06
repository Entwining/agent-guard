#[path = "support/batch6.rs"]
mod contract;

#[test]
fn metadata_operands_do_not_read_credential_contents() {
    contract::partition(
        "control",
        include_str!("fixtures/rust-batch10-metadata.json"),
    );
}

#[test]
fn metadata_roles_preserve_appdata_and_content_read_protection() {
    contract::partition(
        "protected",
        include_str!("fixtures/rust-batch10-metadata.json"),
    );
}
