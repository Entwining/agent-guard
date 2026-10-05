#[path = "support/batch6.rs"]
mod batch6;

#[test]
fn global_pattern_names_follow_the_generic_owner() {
    batch6::partition("global", include_str!("fixtures/rust-batch6.json"));
}

#[test]
fn glued_reader_values_follow_generic_operand_value() {
    batch6::partition("glued", include_str!("fixtures/rust-batch6.json"));
}
