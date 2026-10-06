#[path = "support/batch6.rs"]
mod contract;

#[test]
fn optional_inplace_suffix_preserves_inline_code() {
    contract::partition("protected", include_str!("fixtures/rust-batch10-perl.json"));
}

#[test]
fn inplace_public_code_preserves_interpreter_coverage() {
    contract::partition("control", include_str!("fixtures/rust-batch10-perl.json"));
}
