#[path = "support/batch6.rs"]
mod batch9;

#[test]
fn matrix_candidates_keep_all_shell_read_operands() {
    batch9::partition("matrix", include_str!("fixtures/rust-batch9-matrix.json"));
}

#[test]
fn matrix_summaries_preserve_carried_and_protected_dependencies() {
    batch9::partition("control", include_str!("fixtures/rust-batch9-matrix.json"));
}
