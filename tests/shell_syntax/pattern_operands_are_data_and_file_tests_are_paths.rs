#[test]
fn case_and_test_pattern_operands_are_data() {
    crate::program_contract::partition(
        "pattern-data",
        include_str!("../fixtures/rust-pattern-operands.json"),
    );
}

#[test]
fn file_tests_and_globbing_commands_keep_path_uses() {
    crate::program_contract::partition(
        "file-test",
        include_str!("../fixtures/rust-pattern-operands.json"),
    );
}
