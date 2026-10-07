#[path = "../support/cases/prefix_assignments_use_prior_prefix_values.rs"]
pub mod cases;
use cases::*;

#[test]
fn prefix_assignments_use_prior_prefix_values() {
    contract::partition(
        "prefix",
        include_str!("../fixtures/rust-batch10-parameters.json"),
    );
}
