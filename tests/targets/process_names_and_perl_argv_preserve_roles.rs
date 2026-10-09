use crate::program_contract as contract;

#[test]
fn process_ids_keep_name_roles_and_protected_redirections() {
    contract::partition(
        "kill",
        include_str!("../fixtures/rust-batch17e-programs.json"),
    );
}

#[test]
fn perl_exec_argv_preserves_the_executed_program_roles() {
    contract::partition(
        "perl",
        include_str!("../fixtures/rust-batch17e-programs.json"),
    );
}
