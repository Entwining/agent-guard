use crate::support;
use agent_guard_rust::{Event, adapters, evaluate_with_arm, shell::Arm};
use serde_json::{Value, json};

fn partition(name: &str) {
    let fixture = support::Fixture::new();
    let bytes = if name == "repetition" {
        include_str!("../fixtures/rust-batch17e-array-loops.json")
    } else {
        include_str!("../fixtures/rust-batch17d-arrays.json")
    };
    let packet: Value = serde_json::from_str(bytes).unwrap();
    let rows: Vec<_> = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["partition"] == name)
        .collect();
    assert!(!rows.is_empty());
    for row in rows {
        for zsh in [false, true] {
            for consumer in ["claude", "codex", "pi"] {
                let mut context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
                context.zsh_executor = zsh;
                let source = row[if zsh { "zsh" } else { "bash" }]
                    .as_str()
                    .or_else(|| row["command"].as_str())
                    .unwrap();
                let bytes = serde_json::to_vec(&json!({"tool_name":if consumer == "pi" {"bash"} else {"Bash"},"tool_input":{"command":fixture.expand(source)}})).unwrap();
                let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
                let result = evaluate_with_arm(
                    Event {
                        bytes: &bytes,
                        context: &context,
                        probe: &mut probe,
                    },
                    Arm::Brush,
                );
                let expected = row[if zsh { "expected_zsh" } else { "expected_bash" }]
                    .as_str()
                    .or_else(|| row["expected"].as_str())
                    .unwrap();
                assert_eq!(
                    support::class(&result),
                    expected,
                    "{} {consumer} zsh={zsh}: {result:?}",
                    row["id"]
                );
                let wire = adapters::render(context.consumer, &result);
                assert_eq!(
                    wire.exit,
                    if matches!(expected, "D" | "UR" | "F") {
                        2
                    } else {
                        0
                    }
                );
                assert!(wire.stdout.is_empty());
                assert_eq!(
                    wire.stderr.is_empty(),
                    !matches!(expected, "D" | "UR" | "F")
                );
                if let Some(reason) = row["reason"].as_str() {
                    assert!(
                        wire.stderr.contains(reason),
                        "{}: {}",
                        row["id"],
                        wire.stderr
                    );
                }
                assert_array_observation(&fixture, row, &context, source, zsh);
            }
        }
    }
}

#[test]
fn array_assignment_and_append_preserve_elements() {
    partition("assignment");
}
#[test]
fn array_indices_and_slices_keep_reachable_elements() {
    partition("selection");
}
#[test]
fn array_producers_keep_split_fields_and_lines() {
    partition("producer");
}
#[test]
fn array_copies_calls_and_joins_preserve_scope() {
    partition("scope");
}
#[test]
fn array_expansion_keeps_host_indexing_and_quotes() {
    partition("expansion");
}

#[test]
fn array_generic_read_roles_ignore_element_count() {
    partition("generic");
}

#[test]
fn array_dynamic_width_preserves_quoting_and_dialect() {
    partition("width");
}

#[test]
fn array_repetition_preserves_protected_elements_and_positions() {
    partition("repetition");
}

fn assert_array_observation(
    fixture: &support::Fixture,
    row: &Value,
    context: &agent_guard_rust::Context,
    source: &str,
    zsh: bool,
) {
    if let Some(argv) = row["argv"].as_array() {
        let observation = agent_guard_rust::shell::observe(
            &fixture.expand(source),
            Arm::Brush,
            &context.home,
            &context.cwd,
            zsh,
        )
        .unwrap();
        let command = observation
            .script
            .commands
            .iter()
            .rev()
            .find(|command| command.argv.first().is_some_and(|word| word == "cat"))
            .unwrap();
        assert_eq!(
            command
                .argv
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>(),
            argv.iter()
                .map(|word| word.as_str().unwrap())
                .collect::<Vec<_>>()
        );
    }
    if let Some(unknown) = row["unknown_count"].as_bool() {
        let observation = agent_guard_rust::shell::observe(
            &fixture.expand(source),
            Arm::Brush,
            &context.home,
            &context.cwd,
            zsh,
        )
        .unwrap();
        let command = observation
            .script
            .commands
            .iter()
            .rev()
            .find(|command| command.argv.first().is_some_and(|word| word == "cat"))
            .unwrap();
        assert_eq!(command.argv.last().unwrap().runtime_unknown, unknown);
    }
}
