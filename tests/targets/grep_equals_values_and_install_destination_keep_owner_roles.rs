const ROWS: &str = include_str!("../fixtures/rust-owner-backlog.json");
#[test]
fn grep_equals_values_keep_grep_pattern_file_bytes() {
    crate::program_contract::partition_with("grep", ROWS, |_, evaluation| {
        let agent_guard_rust::Outcome::ProtectedDenial { reason, .. } = &evaluation.outcome else {
            panic!("missing denial");
        };
        assert_eq!(reason.rule, agent_guard_rust::DenialRule::HiddenSearch);
    });
}
#[test]
fn install_destination_keeps_write_role() {
    crate::program_contract::partition_with("install", ROWS, |row, evaluation| {
        if row["id"] == "install-key-destination" {
            assert!(evaluation.effects.iter().any(|effect| matches!(
                effect,
                agent_guard_rust::EffectRecord::ProtectedTarget { write: true, .. }
            )));
        }
    });
}
