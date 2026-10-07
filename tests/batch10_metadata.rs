#[path = "support/batch6.rs"]
mod contract;

#[test]
fn metadata_operands_do_not_read_credential_contents() {
    contract::partition(
        "control",
        include_str!("fixtures/rust-batch10-metadata.json"),
    );
}

#[test]
fn metadata_roles_preserve_appdata_and_content_read_protection() {
    contract::partition_with(
        "protected",
        include_str!("fixtures/rust-batch10-metadata.json"),
        |row, evaluation| {
            use agent_guard_rust::{DenialRule, Outcome};
            let Outcome::ProtectedDenial { reason, .. } = &evaluation.outcome else {
                panic!("missing denial")
            };
            assert_eq!(
                reason.rule,
                if row["id"] == "appdata-metadata" {
                    DenialRule::AppData
                } else {
                    DenialRule::File
                }
            );
            assert!(!reason.rule.message().contains("write"));
        },
    );
}
