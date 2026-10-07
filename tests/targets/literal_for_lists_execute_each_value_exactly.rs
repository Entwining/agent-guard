#[path = "../support/cases/literal_for_lists_execute_each_value_exactly.rs"]
pub mod cases;
use cases::*;

#[test]
fn git_environment_uses_git_directory_roles() {
    partition("git-environment");
}

#[test]
fn bundle_output_is_a_write() {
    contract_rows(&["programs[42]"]);
}

#[test]
fn tar_exclude_is_a_pattern_name() {
    contract_rows(&["programs[64]"]);
}

#[test]
fn rm_operand_is_metadata() {
    contract_rows(&["shell[0]"]);
}

#[test]
fn long_commit_message_is_not_a_probe_fault() {
    contract_rows(&["shell[175]"]);
}
