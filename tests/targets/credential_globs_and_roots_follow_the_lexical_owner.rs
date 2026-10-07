#[path = "../support/cases/credential_globs_and_roots_follow_the_lexical_owner.rs"]
pub mod cases;
use cases::*;

#[test]
fn fd_exec_reads_its_match_root() {
    partition("fd");
}

#[test]
fn git_option_roots_keep_the_go_walk() {
    partition("git");
}

#[test]
fn git_grep_attached_file_values_keep_read_roles() {
    partition("b2b");
}

#[test]
fn tar_glued_directory_is_an_extraction_write_root() {
    partition("tar");
    let fixture = support::Fixture::new();
    for consumer in ["claude", "codex", "pi"] {
        let context = fixture.context(&json!({"consumer":consumer, "cwd":"$P"}));
        let bytes = br#"{"tool_name":"Bash","tool_input":{"command":"tar -xf x.tar -C$HOME/Library/Containers/com.synthetic"}}"#;
        let result = evaluate_with_arm(
            Event {
                bytes,
                context: &context,
                probe: &mut support::RecordingProbe::literal_for_quoted_paths(&fixture),
            },
            Arm::Brush,
        )
        .unwrap();
        let agent_guard_rust::Outcome::ProtectedDenial { reason, .. } = &result.outcome else {
            panic!("missing extraction denial")
        };
        assert_eq!(reason.rule, agent_guard_rust::DenialRule::AppData);
        assert!(reason.effect.contains("write protected location"));
        assert!(result.effects.iter().any(|e| matches!(
            e,
            agent_guard_rust::EffectRecord::ProtectedTarget {
                protection: agent_guard_rust::filesystem::Protection::AppData,
                write: true,
                ..
            }
        )));
    }
}
