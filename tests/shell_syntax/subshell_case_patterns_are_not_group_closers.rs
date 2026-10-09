#[test]
fn subshell_case_patterns_are_not_group_closers() {
    crate::program_contract::partition("case", include_str!("../fixtures/rust-subshell-case.json"));
}
