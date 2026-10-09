use crate::program_contract as batch8;

#[test]
fn nested_functions_follow_dynamic_local_scope() {
    batch8::partition(
        "nested",
        include_str!("../fixtures/rust-batch8-functions.json"),
    );
}

#[test]
fn nested_definitions_activate_when_the_outer_function_runs() {
    batch8::partition(
        "activation",
        include_str!("../fixtures/rust-batch8-functions.json"),
    );
}

#[test]
fn nested_local_restoration_keeps_the_armed_outer_binding() {
    batch8::partition(
        "kept",
        include_str!("../fixtures/rust-batch8-functions.json"),
    );
}

#[test]
fn nested_definitions_keep_ruled_conservative_fallbacks() {
    batch8::partition(
        "fallback",
        include_str!("../fixtures/rust-batch8-functions.json"),
    );
}
