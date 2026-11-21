const ROWS: &str = include_str!("../fixtures/rust-only-known-readers-read-their-operands.json");

#[test]
fn unknown_program_operands_proceed() {
    crate::program_contract::partition("unknown-programs", ROWS);
}

#[test]
fn interpreter_arguments_and_inline_code_proceed() {
    crate::program_contract::partition("interpreter-arguments", ROWS);
}

#[test]
fn known_readers_read_their_operands() {
    crate::program_contract::partition("known-readers", ROWS);
}

#[test]
fn shell_glob_expansion_lists_protected_names_for_any_program() {
    crate::program_contract::partition("shell-expansion", ROWS);
}

#[test]
fn only_modelled_programs_without_a_named_target_enter_the_protected_cwd() {
    crate::program_contract::partition("protected-cwd", ROWS);
}
