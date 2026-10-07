use crate::support;
use agent_guard_rust::{
    EffectRecord, EffectSource, Event, Outcome, adapters, evaluate_with_arm,
    filesystem::Protection,
    shell::{self, Arm},
};
use serde_json::{Value, json};

fn packet() -> Value {
    serde_json::from_str(include_str!("../fixtures/rust-m2-arith-sinks.json")).unwrap()
}

fn partition(name: &str) {
    let fixture = support::Fixture::new();
    let packet = packet();
    let rows: Vec<_> = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["partition"] == name)
        .collect();
    assert!(!rows.is_empty(), "missing partition {name}");
    for row in rows {
        let source = row["source"].as_str().unwrap();
        let observation = shell::observe(source, Arm::Brush, "/h", "/h/project", true).unwrap();
        let nested_read = observation.script.commands.iter().any(|command| {
            command.nested && command.program.is_some_and(|i| command.argv[i] == "cat")
        });
        assert_eq!(nested_read, row["nested"], "{}: {observation:?}", row["id"]);
        for consumer in ["claude", "codex", "pi"] {
            let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
            let bytes = serde_json::to_vec(&json!({"tool_name":if consumer=="pi" {"bash"} else {"Bash"},"tool_input":{"command":source}})).unwrap();
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
                "{consumer}: {}: {result:?}",
                row["id"]
            );
            let wire = adapters::render(context.consumer, &result);
            assert_eq!(
                wire.exit,
                if matches!(row["expected"].as_str(), Some("N" | "UC")) {
                    0
                } else {
                    2
                }
            );
            assert!(wire.stdout.is_empty());
            if row["expected"] == "F" {
                assert_eq!(
                    result.unwrap_err().kind,
                    agent_guard_rust::CheckErrorKind::ResourceLimit
                );
                continue;
            }
            let evaluation = result.unwrap();
            if row["expected"] == "D" {
                assert!(matches!(
                    evaluation.outcome,
                    Outcome::ProtectedDenial { .. }
                ));
                assert!(wire.stderr.contains("credential or environment file"));
                assert!(
                    evaluation.effects.contains(&EffectRecord::ProtectedTarget {
                        protection: Protection::Environment,
                        write: false,
                        source: EffectSource::Nested,
                    }),
                    "{}: {:?}",
                    row["id"],
                    evaluation.effects
                );
            } else if row["expected"] == "N" {
                assert!(wire.stderr.is_empty());
                assert_eq!(
                    support::coverage(&Ok(evaluation))["state"],
                    "SupportedPreflight"
                );
            }
        }
    }
}

#[test]
fn indexed_parameter_expansion_evaluates_binding() {
    partition("index-expansion");
}
#[test]
fn indexed_parameter_length_evaluates_binding() {
    partition("index-length");
}
#[test]
fn substring_offset_evaluates_binding() {
    partition("offset");
}
#[test]
fn substring_length_evaluates_binding() {
    partition("length");
}
#[test]
fn integer_declaration_value_evaluates_binding() {
    partition("integer-init");
}
#[test]
fn integer_later_assignment_evaluates_binding() {
    partition("integer-later");
}
#[test]
fn integer_append_evaluates_binding() {
    partition("integer-append");
}
#[test]
fn indexed_declaration_evaluates_binding() {
    partition("index-declaration");
}
#[test]
fn indexed_unset_evaluates_binding() {
    partition("index-unset");
}
#[test]
fn indexed_assignment_evaluates_binding() {
    partition("index-assignment");
}
#[test]
fn non_armed_binding_behaviour_is_preserved() {
    partition("non-armed");
}
#[test]
fn assignment_references_observe_armed_values_after_calls() {
    partition("attribute-restore");
}
#[test]
fn assignment_reference_is_conservative_after_attribute_removal() {
    partition("attribute-disable");
}
#[test]
fn literal_integer_value_is_not_general_shell_source() {
    partition("integer-data");
}
#[test]
fn accepted_non_executing_references_observe_nested_source() {
    partition("conservative");
}
#[test]
fn existing_arithmetic_sinks_retain_protection() {
    partition("existing");
}
#[test]
fn zsh_witnesses_use_the_general_reference_owner() {
    partition("zsh-sinks");
}
#[test]
fn scope_and_candidate_versions_feed_references() {
    partition("references");
}
#[test]
fn lexer_marks_only_named_subscript_code() {
    partition("lexer-arming");
}
#[test]
fn armed_recursion_is_bounded() {
    partition("bounds");
}
#[test]
fn general_reference_exit() {
    partition("zsh-exit");
}
#[test]
fn general_reference_repeat() {
    partition("zsh-repeat");
}
#[test]
fn general_reference_print() {
    partition("zsh-print");
}
