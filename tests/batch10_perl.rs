#[path = "support/batch6.rs"]
mod contract;

#[test]
fn optional_inplace_suffix_preserves_inline_code() {
    contract::partition_with(
        "protected",
        include_str!("fixtures/rust-batch10-perl.json"),
        |_, evaluation| {
            use agent_guard_rust::{
                DenialRule, EffectRecord, EffectSource, Outcome, filesystem::Protection,
            };
            let Outcome::ProtectedDenial { reason, .. } = &evaluation.outcome else {
                panic!("missing denial")
            };
            assert_eq!(reason.rule, DenialRule::CodeFile);
            assert!(evaluation.effects.contains(&EffectRecord::ProtectedTarget {
                protection: Protection::Environment,
                write: false,
                source: EffectSource::InlineCode
            }));
        },
    );
}

#[test]
fn inplace_public_code_preserves_interpreter_coverage() {
    contract::partition("control", include_str!("fixtures/rust-batch10-perl.json"));
}
