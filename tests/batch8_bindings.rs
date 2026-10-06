#[path = "support/batch6.rs"]
mod batch8;

#[test]
fn substitution_bindings_remain_single_unknown_words() {
    batch8::partition(
        "binding",
        include_str!("fixtures/rust-batch8-bindings.json"),
    );
}

#[test]
fn substitution_binding_controls_keep_protected_targets() {
    batch8::partition(
        "control",
        include_str!("fixtures/rust-batch8-bindings.json"),
    );
}
