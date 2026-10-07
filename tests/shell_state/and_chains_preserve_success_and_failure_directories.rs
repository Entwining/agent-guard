use crate::program_contract as batch8;

#[test]
fn and_chains_preserve_success_and_failure_directories() {
    batch8::partition("chain", include_str!("../fixtures/rust-batch8-chains.json"));
}
