const ROWS: &str = include_str!("../fixtures/rust-function-argv.json");
#[test]
fn definition_argv_is_distinct_from_an_empty_call() {
    crate::program_contract::partition("definition", ROWS);
}
#[test]
fn function_argv_preserves_fields_and_scope() {
    crate::program_contract::partition("argv", ROWS);
}
#[test]
fn shift_advances_the_function_argv() {
    crate::program_contract::partition("shift", ROWS);
}
#[test]
fn omitted_for_list_uses_function_argv() {
    crate::program_contract::partition("for", ROWS);
}
#[test]
fn function_argv_can_supply_the_command_word() {
    crate::program_contract::partition("command", ROWS);
}
