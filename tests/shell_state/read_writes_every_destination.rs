use crate::program_contract as batch8;

#[test]
fn read_writes_every_destination() {
    batch8::partition("replace", include_str!("../fixtures/rust-batch8-read.json"));
}

#[test]
fn read_options_write_the_named_variables() {
    batch8::partition("options", include_str!("../fixtures/rust-batch8-read.json"));
}

#[test]
fn missing_read_option_values_keep_the_public_failure_contract() {
    batch8::partition("control", include_str!("../fixtures/rust-batch8-read.json"));
}
