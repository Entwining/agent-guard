#[path = "support/batch6.rs"]
mod contract;

#[test]
fn prefix_assignments_use_prior_prefix_values() {
    contract::partition(
        "prefix",
        include_str!("fixtures/rust-batch10-parameters.json"),
    );
}

#[test]
fn background_pid_defaults_are_supported_runtime_fragments() {
    contract::partition("pid", include_str!("fixtures/rust-batch10-parameters.json"));
}

#[test]
fn quoted_replacement_patterns_own_their_quotation_context() {
    contract::partition(
        "replacement",
        include_str!("fixtures/rust-batch10-parameters.json"),
    );
}
