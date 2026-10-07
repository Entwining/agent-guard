use crate::program_contract as contract;

#[test]
fn conditional_literal_accumulation_allows_public_output() {
    contract::partition(
        "public",
        include_str!("../fixtures/rust-batch17b-accumulation.json"),
    );
}

#[test]
fn conditional_literal_accumulation_preserves_read_targets() {
    contract::partition(
        "protected",
        include_str!("../fixtures/rust-batch17b-accumulation.json"),
    );
}

#[test]
fn repeated_field_count_is_not_complete_inline_code() {
    contract::partition(
        "code",
        include_str!("../fixtures/rust-batch17b-accumulation.json"),
    );
}

#[test]
fn repeated_field_count_preserves_positional_role_uncertainty() {
    contract::partition(
        "roles",
        include_str!("../fixtures/rust-batch17b-accumulation.json"),
    );
}

#[test]
fn unbounded_literal_accumulation_keeps_budget_refusal() {
    contract::partition_with(
        "budget",
        include_str!("../fixtures/rust-batch17b-accumulation.json"),
        |_, evaluation| {
            assert!(matches!(
                &evaluation.coverage,
                agent_guard_rust::Coverage::LimitedPreflight(gaps)
                    if gaps.contains(&agent_guard_rust::CoverageGap::InspectionBudget)
            ));
        },
    );
}

#[test]
fn repeated_copies_keep_distinct_resource_prefixes() {
    contract::partition(
        "identity",
        include_str!("../fixtures/rust-batch17b-accumulation.json"),
    );
}

#[test]
fn repeated_public_output_keeps_substitution_observation() {
    contract::partition(
        "output",
        include_str!("../fixtures/rust-batch17b-accumulation.json"),
    );
}
