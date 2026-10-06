#[path = "support/batch6.rs"]
mod contract;

#[test]
fn copy_destination_is_written_without_reading_its_contents() {
    contract::partition(
        "destination",
        include_str!("fixtures/rust-batch10c-copy-roles.json"),
    );
}

#[test]
fn copy_sources_and_appdata_destinations_keep_protection() {
    contract::partition(
        "protected",
        include_str!("fixtures/rust-batch10c-copy-roles.json"),
    );
}
