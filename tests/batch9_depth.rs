#[path = "support/batch6.rs"]
mod batch9;

#[test]
fn shell_list_length_is_not_syntactic_nesting() {
    batch9::partition("list", include_str!("fixtures/rust-batch9-depth.json"));
}

#[test]
fn syntactic_nesting_still_fails_closed() {
    batch9::partition("nested", include_str!("fixtures/rust-batch9-depth.json"));
}
