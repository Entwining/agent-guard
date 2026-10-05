#[path = "support/batch6.rs"]
mod batch6;

#[test]
fn tool_names_follow_go_case_folding() {
    batch6::partition("case", include_str!("fixtures/rust-batch6.json"));
}
