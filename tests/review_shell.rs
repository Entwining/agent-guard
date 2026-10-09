mod support;

use agent_guard_rust::{Event, adapters, evaluate_with_arm, shell};
use serde_json::{Value, json};

#[test]
fn runtime_named_directories_keep_protected_suffixes_and_unknown_identity() {
    partition("runtime_named_tilde");
}

#[test]
fn runtime_parameter_defaults_preserve_empty_and_present_candidates() {
    partition("runtime_defaults");
}

#[test]
fn shared_snapshots_preserve_protected_candidates_and_isolated_state() {
    partition("snapshots");
}

fn partition(name: &str) {
    let fixture = support::Fixture::new();
    let packet: Value =
        serde_json::from_str(include_str!("fixtures/rust-review-shell.json")).unwrap();
    let rows: Vec<_> = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["partition"] == name)
        .collect();
    assert!(!rows.is_empty(), "missing review partition {name}");
    for row in rows {
        for consumer in ["claude", "codex", "pi"] {
            let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
            let input = fixture.expand_value(&row["input"]);
            let bytes = serde_json::to_vec(&json!({
                "tool_name":if consumer == "pi" {"bash"} else {"Bash"},
                "tool_input":input
            }))
            .unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate_with_arm(
                Event {
                    bytes: &bytes,
                    context: &context,
                    probe: &mut probe,
                },
                shell::Arm::Brush,
            );
            let expected = row
                .get("expected_pi")
                .filter(|_| consumer == "pi")
                .unwrap_or(&row["expected"]);
            assert_probe_paths(row, &probe);
            assert_eq!(
                support::class(&result),
                expected,
                "{consumer}: {}: {result:?}",
                row["id"]
            );
            let wire = adapters::render(context.consumer, &result);
            let denied = matches!(expected.as_str().unwrap(), "D" | "UR" | "F");
            assert_eq!(
                wire.exit,
                if denied { 2 } else { 0 },
                "{consumer}: {}",
                row["id"]
            );
            assert!(
                wire.stdout.is_empty(),
                "{consumer}: {}: unexpected advice",
                row["id"]
            );
            assert_eq!(wire.stderr.is_empty(), !denied, "{consumer}: {}", row["id"]);
            if denied {
                assert!(
                    wire.stderr.contains(row["reason"].as_str().unwrap()),
                    "{consumer}: {}: {}",
                    row["id"],
                    wire.stderr
                );
                let alternative =
                    row["alternative"].as_str().unwrap_or_else(|| {
                        match row["reason"].as_str().unwrap() {
                            "app-data" => "Name a specific non-sensitive file",
                            "credential or environment file" => {
                                "Otherwise read a non-sensitive config file"
                            }
                            reason => panic!("missing safe alternative for {reason}"),
                        }
                    });
                assert!(
                    wire.stderr.contains(alternative),
                    "missing safe alternative: {}",
                    wire.stderr
                );
            }
            assert_parameter_fields(&fixture, row, &input, &context, consumer);
        }
    }
}

#[test]
fn parameter_defaults_select_known_words() {
    partition("parameter_defaults");
}
#[test]
fn parameter_assignments_reach_later_reads() {
    partition("parameter_assignments");
}
#[test]
fn parameter_alternatives_distinguish_null_and_unset() {
    partition("parameter_alternatives");
}
#[test]
fn parameter_patterns_use_shortest_and_longest_matches() {
    partition("parameter_patterns");
}
#[test]
fn parameter_replacements_preserve_anchoring_and_quotes() {
    partition("parameter_replacements");
}
#[test]
fn parameter_substrings_use_character_offsets() {
    partition("parameter_substrings");
}
#[test]
fn parameter_indirection_reads_the_named_binding() {
    partition("parameter_indirect");
}
#[test]
fn parameter_case_operations_keep_pattern_selection() {
    partition("parameter_case");
}
#[test]
fn parameter_unknown_values_keep_limited_preflight() {
    partition("parameter_unknown");
}

#[test]
fn env_wrappers_apply_assignments_clears_and_unsets() {
    partition("env_wrapper");
}

#[test]
fn bare_cd_uses_current_home_candidates() {
    partition("home_cd");
}

#[test]
fn zsh_parameter_modifiers_share_braced_and_unbraced_semantics() {
    partition("zsh_modifiers");
}

#[test]
fn command_path_modifiers_keep_runtime_lookup_and_protected_reads() {
    partition("command_paths");
}

#[test]
fn tilde_names_use_evaluator_bindings_and_protected_home_suffixes() {
    partition("named_tilde");
}

#[test]
fn wide_loop_header_keeps_a_completed_bounded_result() {
    let fixture = support::Fixture::new();
    let items = (0..2000)
        .map(|n| format!("public{n}"))
        .collect::<Vec<_>>()
        .join(" ");
    for consumer in ["claude", "codex", "pi"] {
        let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
        let bytes = serde_json::to_vec(&json!({"tool_name":"bash","tool_input":{"command":format!("for f in {items}; do printf public; done")}})).unwrap();
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        let result = evaluate_with_arm(
            Event {
                bytes: &bytes,
                context: &context,
                probe: &mut probe,
            },
            shell::Arm::Brush,
        );
        assert_eq!(support::class(&result), "UR", "{result:?}");
        let wire = adapters::render(context.consumer, &result);
        assert_eq!(wire.exit, 2);
        assert!(wire.stdout.is_empty());
        assert!(wire.stderr.contains("inspection budget"), "{}", wire.stderr);
        assert!(
            wire.stderr
                .contains("Split loops, function calls or brace alternatives")
        );
        assert!(wire.stderr.contains("recheck"), "{}", wire.stderr);
    }
}

#[test]
fn quoted_delimiters_are_data_and_real_nesting_stays_bounded() {
    let fixture = support::Fixture::new();
    let mut sources = Vec::new();
    for delimiter in ['[', '{', '('] {
        for quote in ['\'', '"'] {
            sources.push(format!(
                "printf '%s' {quote}{}{quote}",
                delimiter.to_string().repeat(65)
            ));
        }
    }
    sources.push(format!("printf public # {}", "[".repeat(65)));
    sources.push(format!("printf public <<TAG\n{}\nTAG", "{".repeat(65)));
    for source in sources {
        for consumer in ["claude", "codex", "pi"] {
            let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
            let bytes =
                serde_json::to_vec(&json!({"tool_name":"bash","tool_input":{"command":source}}))
                    .unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate_with_arm(
                Event {
                    bytes: &bytes,
                    context: &context,
                    probe: &mut probe,
                },
                shell::Arm::Brush,
            );
            assert_eq!(support::class(&result), "N", "{source}: {result:?}");
            let wire = adapters::render(context.consumer, &result);
            assert_eq!(wire.exit, 0);
            assert!(wire.stdout.is_empty() && wire.stderr.is_empty());
        }
    }
    assert!(shell::check_nesting(&format!("{}true{}", "(".repeat(64), ")".repeat(64))).is_ok());
    assert_eq!(
        shell::check_nesting(&format!("{}true{}", "(".repeat(65), ")".repeat(65)))
            .unwrap_err()
            .kind,
        agent_guard_rust::CheckErrorKind::ResourceLimit
    );
    for consumer in ["claude", "codex", "pi"] {
        let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
        let bytes = serde_json::to_vec(&json!({"tool_name":"bash","tool_input":{"command":format!("{}true{}", "(".repeat(65), ")".repeat(65))}})).unwrap();
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        let result = evaluate_with_arm(
            Event {
                bytes: &bytes,
                context: &context,
                probe: &mut probe,
            },
            shell::Arm::Brush,
        );
        assert_eq!(
            result.as_ref().unwrap_err().kind,
            agent_guard_rust::CheckErrorKind::ResourceLimit
        );
        let wire = adapters::render(context.consumer, &result);
        assert_eq!(wire.exit, 2);
        assert!(wire.stdout.is_empty());
        assert!(wire.stderr.contains("within its resource limit"));
        assert!(wire.stderr.contains("Split the work into smaller calls"));
        assert!(wire.stderr.contains("recheck each call"));
    }
}

fn assert_probe_paths(row: &Value, probe: &support::RecordingProbe) {
    if let Some(name) = row["no_private_probe"].as_str() {
        assert!(
            probe
                .calls
                .iter()
                .all(|path| !path.split('/').any(|part| part == name))
                && probe.stat_calls.is_empty(),
            "{}: {:?} {:?}",
            row["id"],
            probe.calls,
            probe.stat_calls
        );
    }
    if let Some(suffix) = row["no_protected_probe_suffix"].as_str() {
        assert!(
            probe
                .calls
                .iter()
                .chain(&probe.stat_calls)
                .all(|path| !path.ends_with(suffix)),
            "{}: protected probe: {:?} {:?}",
            row["id"],
            probe.calls,
            probe.stat_calls
        );
    }
}

fn assert_parameter_fields(
    fixture: &support::Fixture,
    row: &Value,
    input: &Value,
    context: &agent_guard_rust::Context,
    consumer: &str,
) {
    if let Some(word) = row["word"].as_str() {
        let observed = shell::observe_with_user(
            input["command"].as_str().unwrap(),
            shell::Arm::Brush,
            &context.home,
            &context.cwd,
            context.user.as_deref(),
            context.zsh_executor,
        )
        .unwrap();
        let expected = fixture.expand(word);
        assert!(
            observed
                .script
                .commands
                .iter()
                .any(
                    |command| command.program.is_some_and(|i| command.argv[i] == "cat"
                        && command
                            .argv
                            .last()
                            .is_some_and(|word| word.text == expected))
                ),
            "{consumer}: {}: missing computed word {expected:?}: {:?}",
            row["id"],
            observed.script
        );
    }
    if let Some(field) = row["field_zsh"].as_str().filter(|_| consumer != "pi") {
        let expected = fixture.expand(field);
        let prefix = expected.split(' ').next().unwrap();
        let observed = shell::observe_with_user(
            input["command"].as_str().unwrap(),
            shell::Arm::Brush,
            &context.home,
            &context.cwd,
            context.user.as_deref(),
            context.zsh_executor,
        )
        .unwrap();
        let commands: Vec<_> = observed
            .script
            .commands
            .iter()
            .filter(|command| {
                command
                    .program
                    .is_some_and(|index| command.argv[index] == "cat")
                    && command
                        .argv
                        .iter()
                        .any(|word| word.text.starts_with(prefix))
            })
            .collect();
        assert!(!commands.is_empty(), "{}", row["id"]);
        for command in commands {
            let index = command.program.unwrap();
            assert_eq!(
                command.argv.len(),
                index + 2,
                "{}: {:?}",
                row["id"],
                command.argv
            );
            assert_eq!(command.argv[index + 1].text, expected, "{}", row["id"]);
        }
    }
}
