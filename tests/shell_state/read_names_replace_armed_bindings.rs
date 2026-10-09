use crate::program_contract as batch7;

#[test]
fn read_names_replace_armed_bindings() {
    batch7::partition("read", include_str!("../fixtures/rust-batch7-writers.json"));
}

#[test]
fn printf_destination_replaces_armed_binding() {
    batch7::partition(
        "printf",
        include_str!("../fixtures/rust-batch7-writers.json"),
    );
}

#[test]
fn function_local_names_replace_and_restore_bindings() {
    batch7::partition(
        "local",
        include_str!("../fixtures/rust-batch7-writers.json"),
    );
}

#[test]
fn writer_models_preserve_other_armed_consumptions() {
    batch7::partition("kept", include_str!("../fixtures/rust-batch7-writers.json"));
}
