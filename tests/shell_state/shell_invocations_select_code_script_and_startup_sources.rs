use crate::program_contract as contract;

#[test]
fn shell_invocations_select_code_script_and_startup_sources() {
    contract::partition(
        "shell-sources",
        include_str!("../fixtures/rust-shell-invocation-sources.json"),
    );
}
