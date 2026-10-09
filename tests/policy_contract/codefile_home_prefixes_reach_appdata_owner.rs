use crate::program_contract as batch9;

#[test]
fn codefile_home_prefixes_reach_appdata_owner() {
    batch9::partition_with(
        "protected",
        include_str!("../fixtures/rust-batch9-home.json"),
        |_, evaluation| {
            use agent_guard_rust::{
                DenialRule, EffectRecord, EffectSource, Outcome, filesystem::Protection,
            };
            let Outcome::ProtectedDenial { reason, .. } = &evaluation.outcome else {
                panic!("missing denial")
            };
            assert_eq!(reason.rule, DenialRule::AppData);
            assert!(reason.effect.starts_with("CodeFile:"));
            assert!(evaluation.effects.contains(&EffectRecord::ProtectedTarget {
                protection: Protection::AppData,
                write: false,
                source: EffectSource::InlineCode
            }));
        },
    );
}

#[test]
fn codefile_home_prefixes_preserve_public_and_broad_controls() {
    batch9::partition("control", include_str!("../fixtures/rust-batch9-home.json"));
}
