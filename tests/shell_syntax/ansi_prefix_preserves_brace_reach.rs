use crate::support;
use agent_guard_rust::{
    CoverageGap, Event, Outcome, evaluate_with_arm,
    shell::{self, Arm},
};
use serde_json::{Value, json};

fn regressions(owner: &str) {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("../fixtures/rust-m1-2-regressions.json")).unwrap();
    let fixture = support::Fixture::new();
    for consumer in ["claude", "codex", "pi"] {
        let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
        for row in rows.iter().filter(|row| row["owner"] == owner) {
            if let Some(divergent) = row["divergent"].as_bool() {
                let observation = shell::observe_with_user(
                    row["source"].as_str().unwrap(),
                    Arm::Brush,
                    &fixture.home,
                    &fixture.project,
                    context.user.as_deref(),
                    context.zsh_executor,
                )
                .unwrap();
                assert_eq!(
                    observation.gaps.iter().any(|gap| matches!(
                        gap,
                        CoverageGap::ExecutorDivergence | CoverageGap::UnsupportedDialectConstruct
                    )),
                    divergent,
                    "literal heredoc body vs active tail: {row}"
                );
            }
            let event = if let Some(source) = row["source"].as_str() {
                json!({"tool_name":if consumer == "pi" {"bash"} else {"Bash"},
                    "tool_input":{"command":fixture.expand(source)}})
            } else if consumer == "pi" {
                json!({"tool_name":"read","tool_input":{"path":fixture.expand(row["path"].as_str().unwrap())}})
            } else {
                json!({"tool_name":"Read","tool_input":{"file_path":fixture.expand(row["path"].as_str().unwrap())}})
            };
            let bytes = serde_json::to_vec(&event).unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate_with_arm(
                Event {
                    bytes: &bytes,
                    context: &context,
                    probe: &mut probe,
                },
                Arm::Brush,
            );
            assert_eq!(
                support::class(&result),
                row["expected"],
                "{consumer}: {row}"
            );
            let wire = agent_guard_rust::adapters::render(context.consumer, &result);
            assert_eq!(
                wire.exit,
                if matches!(row["expected"].as_str(), Some("D" | "UR")) {
                    2
                } else {
                    0
                }
            );
            assert!(wire.stdout.is_empty(), "no advice expected: {row}");
            if row["expected"] == "D" {
                let Outcome::ProtectedDenial { reason, recovery } =
                    &result.as_ref().unwrap().outcome
                else {
                    panic!("missing protected denial: {row}");
                };
                let protection = match row["id"].as_str().unwrap() {
                    "ansi-appdata-list" => "App Data",
                    "ansi-npmrc-list" => "credential-file",
                    _ => "environment-file",
                };
                assert!(reason.effect.contains(protection), "{row}: {reason:?}");
                assert!(!recovery.excluded_scope.is_empty());
            }
        }
    }
}

#[test]
fn ansi_prefix_preserves_brace_reach() {
    regressions("brace_text");
}
#[test]
fn ansi_member_preserves_brace_reach() {
    regressions("brace_group");
}
#[test]
fn ansi_literal_is_inert_to_divergence() {
    regressions("divergence_literal");
}
#[test]
fn ansi_qualifier_keeps_parenthesis_pairing() {
    regressions("divergence_closing");
}
#[test]
fn quoted_heredoc_body_is_inert_to_divergence() {
    regressions("divergence_heredoc");
}
#[test]
fn credential_directory_walk_requires_read() {
    regressions("credential_effect");
    let fixture = support::Fixture::new();
    for source in [
        "rg needle ~/.docker",
        "ls -R ~/Library/Containers",
        "cat ~/.docker/config.json",
    ] {
        let context = fixture.context(&json!({"consumer":"claude","cwd":"$P"}));
        let bytes =
            serde_json::to_vec(&json!({"tool_name":"Bash","tool_input":{"command":source}}))
                .unwrap();
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        assert_eq!(
            support::class(&evaluate_with_arm(
                Event {
                    bytes: &bytes,
                    context: &context,
                    probe: &mut probe
                },
                Arm::Brush
            )),
            "D",
            "{source}"
        );
    }
}
#[test]
fn literal_braces_keep_their_path_identity() {
    regressions("literal_brace");
}
