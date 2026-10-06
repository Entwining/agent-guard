#[path = "support/batch6.rs"]
mod batch8;

#[test]
fn curl_urls_after_double_dash_keep_their_read_role() {
    batch8::partition("url", include_str!("fixtures/rust-batch8-curl.json"));
}

#[test]
fn curl_suffix_urls_do_not_become_options() {
    batch8::partition("control", include_str!("fixtures/rust-batch8-curl.json"));
}
