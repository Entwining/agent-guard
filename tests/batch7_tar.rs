#[path = "support/batch6.rs"]
mod batch7;

#[test]
fn tar_short_values_keep_their_read_or_write_roles() {
    batch7::partition("tar", include_str!("fixtures/rust-batch7-tar.json"));
    batch7::partition("archive", include_str!("fixtures/rust-batch7-tar.json"));
    batch7::partition("directory", include_str!("fixtures/rust-batch7-tar.json"));
}
