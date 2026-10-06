#[path = "support/batch6.rs"]
mod batch8;

#[test]
fn ctags_owns_its_option_file_references() {
    batch8::partition("ctags", include_str!("fixtures/rust-batch8-atfile.json"));
}

#[test]
fn global_option_at_values_are_plain_names() {
    batch8::partition("names", include_str!("fixtures/rust-batch8-atfile.json"));
}

#[test]
fn ctags_patterns_and_suffix_operands_keep_their_roles() {
    batch8::partition("control", include_str!("fixtures/rust-batch8-atfile.json"));
}
