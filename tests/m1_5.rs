mod support;

use agent_guard_rust::{
    CoverageGap, EffectRecord, EffectSource, Event, Outcome, adapters, evaluate_with_arm,
    shell::{Arm, lexer::Lexed},
};
use serde_json::{Value, json};

fn rows(owner: &str) {
    let mut rows: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/rust-m1-5-regressions.json")).unwrap();
    rows.extend(
        serde_json::from_str::<Vec<Value>>(include_str!("fixtures/rust-m1-5-forwarding.json"))
            .unwrap(),
    );
    let fixture = support::Fixture::new();
    let mut failures = Vec::new();
    for consumer in ["claude", "codex", "pi"] {
        let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
        for row in rows.iter().filter(|r| r["owner"] == owner) {
            let source = row["source"].as_str().unwrap();
            let bytes = serde_json::to_vec(&json!({"tool_name":if consumer == "pi" {"bash"} else {"Bash"},"tool_input":{"command":source}})).unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate_with_arm(
                Event {
                    bytes: &bytes,
                    context: &context,
                    probe: &mut probe,
                },
                Arm::Brush,
            );
            let wire = adapters::render(context.consumer, &result);
            let class = support::class(&result);
            println!(
                "{}",
                json!({"id":row["id"],"consumer":consumer,"class":class,"wire":format!("{wire:?}"),"result":format!("{result:?}")})
            );
            if class != row["expected"] {
                failures.push(row["id"].clone());
                continue;
            }
            assert_eq!(wire.exit, if ["N", "UC"].contains(&class) { 0 } else { 2 });
            assert!(wire.stdout.is_empty(), "absent advice: {row}");
            match &result.as_ref().unwrap().outcome {
                Outcome::NoObjection => assert!(wire.stderr.is_empty()),
                Outcome::ProtectedDenial { reason, recovery } => {
                    assert!(
                        reason.effect.contains(row["reason"].as_str().unwrap()),
                        "denial reason: {row}"
                    );
                    assert!(wire.stderr.contains(&reason.effect));
                    assert!(!recovery.excluded_scope.is_empty());
                    assert!(wire.stderr.contains("recheck"));
                    if owner == "forwarding" {
                        let protection = if row["reason"] == "private-key" {
                            agent_guard_rust::filesystem::Protection::SshPrivate
                        } else {
                            agent_guard_rust::filesystem::Protection::Environment
                        };
                        assert!(
                            result.as_ref().unwrap().effects.contains(
                                &EffectRecord::ProtectedTarget {
                                    protection,
                                    write: false,
                                    source: if row["id"] == "g14-subshell" {
                                        EffectSource::Operand
                                    } else {
                                        EffectSource::Nested
                                    }
                                }
                            ),
                            "nested protected effect: {row}"
                        );
                    }
                }
                Outcome::CoverageInsufficient {
                    cause, recovery, ..
                } => {
                    if row["reason"] == "syntax" {
                        assert_eq!(cause, &CoverageGap::UnsupportedShellSyntax);
                    } else if row["reason"] == "unknown" {
                        assert!(matches!(cause, CoverageGap::UnknownProgram { .. }));
                    } else {
                        assert_eq!(
                            cause,
                            &if consumer == "pi" {
                                CoverageGap::UnsupportedDialectConstruct
                            } else {
                                CoverageGap::ExecutorDivergence
                            }
                        );
                    }
                    if class == "UR" {
                        assert!(wire.stderr.contains("unsupported"));
                        assert!(wire.stderr.contains("recheck"));
                        assert!(recovery.is_some());
                    }
                    assert!(!wire.stderr.contains("checker failed"));
                }
                other => panic!("unexpected advice/outcome: {other:?}"),
            }
            if owner == "redirect" {
                let lexical = Lexed::scan(source).unwrap();
                assert!(
                    lexical.context(source.find('#').unwrap()).comment,
                    "redirect comment context: {row}"
                );
            }
            if owner == "array" && source.contains("a=(x)#") {
                let lexical = Lexed::scan(source).unwrap();
                assert!(
                    !lexical.context(source.find('#').unwrap()).comment,
                    "literal Bash reading must remain visible: {row}"
                );
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{owner} contract failures: {failures:?}"
    );
}

#[test]
fn array_closer_divergence_blocks_both_host_readings() {
    rows("array");
}
#[test]
fn option_builtins_are_blocked_at_the_adapter() {
    rows("adapter");
}
#[test]
fn redirect_hash_is_a_comment_and_incomplete() {
    rows("redirect");
}

#[test]
fn raw_detector_retirement_keeps_nested_command() {
    rows("raw-retirement");
}

#[test]
fn lexical_substitution_body_keeps_protected_next_line() {
    rows("forwarding");
}

#[test]
fn array_tail_has_its_own_lexical_span() {
    for source in [
        "a=(x)#X",
        "a+=(x)#X",
        "typeset a=(x)#X",
        "a=(x)\"y\"",
        "a=(x) b=(y)#X",
    ] {
        let lexical = Lexed::scan(source).unwrap();
        let closer = source.rfind(')').unwrap();
        assert_eq!(
            lexical.array_tail_spans(),
            vec![closer..source.len()],
            "{source}"
        );
    }
    for source in [
        "a=(x y); echo ${a[1]}",
        "a=(x)>out",
        "(a=(x))",
        "a=(x) #X",
        "echo $(echo hi)#X",
    ] {
        assert!(
            Lexed::scan(source).unwrap().array_tail_spans().is_empty(),
            "{source}"
        );
    }
}
