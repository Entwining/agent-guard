use crate::support;
use agent_guard_rust::{Event, Outcome, adapters, evaluate_with_arm, shell::Arm};
use serde_json::{Value, json};
use std::path::Path;

#[test]
fn escaped_home_keeps_broad_root_protection() {
    check_rows(|row| row["id"].as_str().unwrap().starts_with('v'));
}

#[test]
fn encoded_literals_preserve_protection_after_brace_expansion() {
    check_rows(|row| row["id"] == "brace-appdata-literal");
}

#[test]
fn quoted_credential_directories_keep_protection_with_wildcard_children() {
    check_rows(|row| row["id"] == "quoted-credential-directory-glob");
}

#[test]
fn brace_sequences_reach_only_their_elements() {
    check_rows(|row| row["id"].as_str().unwrap().starts_with("sequence-"));
}

#[test]
fn quoted_literals_do_not_create_recursive_home_scans() {
    check_rows(|row| row["expected"] == "N");
}

fn check_rows(select: impl Fn(&Value) -> bool) {
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join("home-x+y@z");
    let fixture = support::Fixture::new_at(&scratch);
    let packet: Value =
        serde_json::from_str(include_str!("../fixtures/rust-broad-escaped-home.json")).unwrap();
    for row in packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| select(row))
    {
        for consumer in ["claude", "codex", "pi"] {
            let context = fixture.context(&json!({"consumer":consumer,"cwd":row["cwd"]}));
            let bytes = serde_json::to_vec(&json!({
                "tool_name":if consumer == "pi" { "bash" } else { "Bash" },
                "tool_input":row["input"]
            }))
            .unwrap();
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
                "{consumer} {}: {result:?}",
                row["id"]
            );
            let wire = adapters::render(context.consumer, &result);
            assert_eq!(wire.exit, if row["expected"] == "D" { 2 } else { 0 });
            assert!(
                wire.stdout.is_empty(),
                "{consumer} {}: unsolicited advice",
                row["id"]
            );
            let evaluation = result.unwrap();
            let effects = adapters::effects_value(&evaluation.effects);
            let mut expected_effects = Vec::new();
            if row["broad"] == true {
                expected_effects.push(json!({"kind":"BroadRoot"}));
            }
            if !row["protection"].is_null() {
                expected_effects.push(json!({
                    "kind":"ProtectedTarget", "protection":row["protection"],
                    "source":"Operand", "write":false
                }));
            }
            assert_eq!(
                effects,
                json!(expected_effects),
                "{consumer} {} effects",
                row["id"]
            );
            if row["expected"] == "N" {
                assert!(wire.stderr.is_empty());
            } else {
                let Outcome::ProtectedDenial { reason, recovery } = &evaluation.outcome else {
                    panic!("expected protected denial");
                };
                assert!(wire.stderr.contains(reason.rule.message()));
                assert!(!recovery.automatic_application_supported);
                if row["broad"] == true && row["protection"].is_null() {
                    assert!(wire.stderr.contains("Scope the scan to a project path."));
                    let recovery = adapters::recovery_value(recovery);
                    assert_eq!(recovery["next_step"]["kind"], "owner_action");
                    let excluded = recovery["excluded_scope"].to_string();
                    for scope in ["HOME", "outside", "Library", ".ssh", "environment-file"] {
                        assert!(
                            excluded.contains(scope),
                            "{consumer} {}: {recovery}",
                            row["id"]
                        );
                    }
                }
            }
        }
    }
}
