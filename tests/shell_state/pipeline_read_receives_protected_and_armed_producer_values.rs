use crate::program_contract as batch9;

#[test]
fn pipeline_read_preserves_options_public_unknown_and_bash_state() {
    batch9::partition("control", include_str!("../fixtures/rust-batch9-read.json"));
}
