use crate::support;

use agent_guard_rust::{
    Coverage, EffectRecord, EffectSource, Event, Outcome, adapters, evaluate,
    filesystem::Protection, shell,
};
use serde_json::{Value, json};

fn rows(owner: &str, expected: &[&str]) {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("../fixtures/rust-m2-step0.json")).unwrap();
    let fixture = support::Fixture::new();
    for consumer in ["claude", "codex", "pi"] {
        let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
        let selected: Vec<_> = rows
            .iter()
            .filter(|r| r["owner"] == owner && expected.contains(&r["expected"].as_str().unwrap()))
            .collect();
        assert!(
            !selected.is_empty(),
            "missing owner {owner} with outcomes {expected:?}"
        );
        for row in selected {
            let source = row["source"].as_str().unwrap();
            let input = |source: &str| {
                serde_json::to_vec(&json!({
                    "tool_name": if consumer == "pi" { "bash" } else { "Bash" },
                    "tool_input":{"command":source},
                }))
                .unwrap()
            };
            let bytes = input(source);
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate(Event {
                bytes: &bytes,
                context: &context,
                probe: &mut probe,
            });
            let wire = adapters::render(context.consumer, &result);
            let class = support::class(&result);
            println!(
                "{}",
                json!({"id":row["id"],"consumer":consumer,"class":class,"result":format!("{result:?}"),"wire":format!("{wire:?}")})
            );
            match row["expected"].as_str().unwrap() {
                "data-control" => {
                    let bytes = input(row["control"].as_str().unwrap());
                    let control = evaluate(Event {
                        bytes: &bytes,
                        context: &context,
                        probe: &mut probe,
                    });
                    assert_eq!(
                        class,
                        support::class(&control),
                        "heredoc data reading: {row}"
                    );
                    let observation =
                        shell::observe(source, &fixture.home, &fixture.project, true).unwrap();
                    assert!(
                        !observation
                            .script
                            .commands
                            .iter()
                            .any(|c| c.argv.first().is_some_and(|w| w.text == "cat")),
                        "body text is not a command: {row}"
                    );
                }
                expected => assert_eq!(class, expected, "{consumer}: {row}"),
            }
            assert_eq!(wire.exit, if matches!(class, "N" | "UC") { 0 } else { 2 });
            if matches!(row["expected"].as_str(), Some("N" | "D")) {
                assert_eq!(
                    result.as_ref().unwrap().coverage,
                    Coverage::SupportedPreflight,
                    "complete observed syntax has no incidental refusal: {row}"
                );
            }
            assert!(wire.stdout.is_empty(), "no advice: {row}");
            assert_blank_outcome(row, &result);
            assert!(!wire.stderr.contains("checker failed"));
        }
    }
}

#[test]
fn shell_word_blanks_keep_benign_permits() {
    rows("word-blank", &["N"]);
}
#[test]
fn shell_word_blanks_observe_protected_nested_code() {
    rows("word-blank", &["D"]);
}
#[test]
fn heredoc_delimiter_blanks_keep_benign_permits() {
    rows("heredoc-blank", &["N"]);
}
#[test]
fn heredoc_delimiter_blanks_observe_protected_code() {
    rows("heredoc-blank", &["D"]);
}
#[test]
fn heredoc_delimiter_blanks_keep_data_and_wrong_terminators_keep_later_lines_in_the_body() {
    rows("heredoc-blank", &["data-control", "UC"]);
}

fn assert_blank_outcome(
    row: &Value,
    result: &Result<agent_guard_rust::Evaluation, agent_guard_rust::CheckError>,
) {
    match &result.as_ref().unwrap().outcome {
        Outcome::ProtectedDenial { reason, .. } => {
            assert!(
                reason.effect.contains("environment-file"),
                "Environment owner: {row}"
            );
            assert!(
                result
                    .as_ref()
                    .unwrap()
                    .effects
                    .contains(&EffectRecord::ProtectedTarget {
                        protection: Protection::Environment,
                        write: false,
                        source: if row["effect_source"] == "nested" {
                            EffectSource::Nested
                        } else {
                            EffectSource::Operand
                        },
                    }),
                "effect provenance: {row}"
            );
        }
        Outcome::CoverageInsufficient { .. } | Outcome::NoObjection => {}
        other => panic!("unexpected outcome for {row}: {other:?}"),
    }
}
