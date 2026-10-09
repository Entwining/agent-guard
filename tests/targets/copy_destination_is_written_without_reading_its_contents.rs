use crate::program_contract as contract;

#[test]
fn credential_copy_destinations_refuse_resource_changes() {
    contract::partition_with(
        "destination",
        include_str!("../fixtures/rust-batch10c-copy-roles.json"),
        |_, evaluation| {
            let agent_guard_rust::Outcome::ProtectedDenial { reason, .. } = &evaluation.outcome
            else {
                panic!("missing change denial")
            };
            assert_eq!(reason.rule, agent_guard_rust::DenialRule::ResourceChange);
        },
    );
}

#[test]
fn copy_sources_and_appdata_destinations_keep_protection() {
    contract::partition(
        "protected",
        include_str!("../fixtures/rust-batch10c-copy-roles.json"),
    );
}
