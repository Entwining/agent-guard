#[path = "support/batch6.rs"]
mod batch7;

#[test]
fn global_atfile_values_read_and_plain_values_remain_names() {
    batch7::partition("atfile", include_str!("fixtures/rust-batch7-atfile.json"));
    batch7::partition("name", include_str!("fixtures/rust-batch7-atfile.json"));
}
