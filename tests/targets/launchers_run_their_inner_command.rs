const ROWS: &str = include_str!("../fixtures/rust-launcher-roles.json");

#[test]
fn launcher_env_files_are_used_not_read() {
    crate::program_contract::partition("env-file", ROWS);
}

#[test]
fn launchers_run_their_inner_command() {
    crate::program_contract::partition("inner", ROWS);
}

#[test]
fn launcher_directory_options_set_the_inner_cwd() {
    crate::program_contract::partition("directory", ROWS);
}

#[test]
fn wrapper_directories_leave_redirects_and_earlier_files_at_the_shell_cwd() {
    crate::program_contract::partition("shell-cwd", ROWS);
}

#[test]
fn launcher_path_values_are_used() {
    crate::program_contract::partition("path-value", ROWS);
}

#[test]
fn unknown_launcher_options_keep_the_unknown_program_reads() {
    crate::program_contract::partition("fallback", ROWS);
}

#[test]
fn wrapper_argument_files_are_read() {
    crate::program_contract::partition("wrapper-file", ROWS);
}

#[test]
fn interpreter_env_files_are_used_and_inline_code_is_inspected() {
    crate::program_contract::partition("interpreter-env-file", ROWS);
}
