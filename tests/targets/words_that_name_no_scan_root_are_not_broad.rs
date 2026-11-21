use crate::program_contract as contract;

const ROWS: &str = include_str!("../fixtures/rust-non-scan-roots.json");

#[test]
fn words_inside_unknown_substitutions_are_not_operands() {
    contract::partition("substitution", ROWS);
}

#[test]
fn operands_beside_unknown_substitutions_keep_their_roles() {
    contract::partition("substitution-operand", ROWS);
}

#[test]
fn git_directories_of_naming_subcommands_are_not_walked() {
    contract::partition("git-names", ROWS);
}

#[test]
fn git_history_subcommands_read_named_paths_without_walking() {
    contract::partition("git-history", ROWS);
}

#[test]
fn git_directories_of_work_tree_subcommands_are_walked() {
    contract::partition("git-walk", ROWS);
}
