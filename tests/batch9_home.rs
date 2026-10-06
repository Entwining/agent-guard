#[path = "support/batch6.rs"]
mod batch9;

#[test]
fn codefile_home_prefixes_reach_appdata_owner() {
    batch9::partition("protected", include_str!("fixtures/rust-batch9-home.json"));
}

#[test]
fn codefile_home_prefixes_preserve_public_and_broad_controls() {
    batch9::partition("control", include_str!("fixtures/rust-batch9-home.json"));
}
