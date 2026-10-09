#[path = "support/batch6.rs"]
mod batch6;
mod support;

use agent_guard_rust::{Coverage, CoverageGap};

fn partition(name: &str) {
    batch6::partition_with(
        name,
        include_str!("fixtures/rust-runtime-default-cost.json"),
        |_, evaluation| {
            let Coverage::LimitedPreflight(gaps) = &evaluation.coverage else {
                panic!("runtime output must retain limited preflight");
            };
            assert!(!gaps.contains(&CoverageGap::InspectionBudget));
        },
    );
}

#[test]
fn polling_defaults_preserve_limited_preflight_without_candidate_explosion() {
    partition("poll");
}

#[test]
fn background_function_defaults_preserve_limited_preflight() {
    partition("background");
}

#[test]
fn default_projections_preserve_nested_and_later_protected_reads() {
    batch6::partition(
        "projection",
        include_str!("fixtures/rust-runtime-default-cost.json"),
    );
}
