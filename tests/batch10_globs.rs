#[path = "support/batch6.rs"]
mod contract;

#[test]
fn ordinary_directory_globs_do_not_intersect_wildcard_only_credential_tails() {
    contract::partition("control", include_str!("fixtures/rust-batch10-globs.json"));
}

#[test]
fn credential_directory_and_file_globs_still_refuse() {
    contract::partition(
        "protected",
        include_str!("fixtures/rust-batch10-globs.json"),
    );
}
