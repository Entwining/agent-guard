#[path = "support/batch6.rs"]
mod batch7;

#[test]
fn tool_names_follow_go_simple_rune_mapping() {
    batch7::partition("case", include_str!("fixtures/rust-batch7-case.json"));
}
