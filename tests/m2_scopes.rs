mod support;
use agent_guard_rust::{Event, adapters, evaluate_with_arm, shell::Arm};
use serde_json::{Value, json};

fn rows() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("fixtures/rust-m2-scopes.json")).unwrap()["rows"]
        .as_array()
        .unwrap()
        .clone()
}

fn partition(name: &str) {
    let fixture = support::Fixture::new();
    for row in rows().into_iter().filter(|r| r["partition"] == name) {
        for consumer in ["claude", "codex", "pi"] {
            let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
            let bytes = serde_json::to_vec(&json!({"tool_name":if consumer=="pi" {"bash"} else {"Bash"},"tool_input":{"command":row["source"]}})).unwrap();
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
                "{consumer}: {} {}: {result:?}",
                row["id"],
                row["scope"]
            );
            let wire = adapters::render(context.consumer, &result);
            assert_eq!(wire.exit, if row["expected"] == "N" { 0 } else { 2 });
            assert!(wire.stdout.is_empty());
            if row["expected"] == "D" {
                assert!(wire.stderr.contains("protected"));
                assert!(
                    wire.stderr
                        .contains(row["reason"].as_str().unwrap_or("App Data")),
                    "{}: {}",
                    row["id"],
                    wire.stderr
                );
                assert!(matches!(
                    result,
                    Ok(agent_guard_rust::Evaluation {
                        outcome: agent_guard_rust::Outcome::ProtectedDenial { .. },
                        ..
                    })
                ));
            } else if row["expected"] == "N" {
                assert!(wire.stderr.is_empty());
                assert_eq!(support::coverage(&result)["state"], "SupportedPreflight");
            }
        }
    }
}

#[test]
fn persistent_binding_eligibility() {
    partition("binding");
}
#[test]
fn compound_words_are_use_targets() {
    partition("use");
}
#[test]
fn function_local_after_return_is_benign() {
    partition("function-return");
}
#[test]
fn function_return_restores_shadowed_binding() {
    partition("function-restore");
}
#[test]
fn function_local_is_read_inside_body() {
    partition("function-local");
}
#[test]
fn function_dynamic_scope_reads_caller_local() {
    partition("function-dynamic");
}
#[test]
fn function_plain_assignment_is_global() {
    partition("function-global");
}
#[test]
fn function_arithmetic_uses_local_binding() {
    partition("function-arithmetic");
}
#[test]
fn function_evaluated_variable_uses_local_binding() {
    partition("function-evaluated");
}
#[test]
fn isolated_scopes_do_not_mutate_caller() {
    partition("isolation");
}
#[test]
fn brace_group_moves_parent() {
    partition("group");
}
#[test]
fn pipeline_scopes_preserve_dialect_reach() {
    partition("pipeline");
}
#[test]
fn conditional_bindings_merge_reachable_exits() {
    partition("if-join");
}
#[test]
fn case_bindings_merge_arms_and_no_match() {
    partition("case-join");
}
#[test]
fn loop_bindings_merge_zero_iteration_and_early_exits() {
    partition("loop-join");
}
#[test]
fn and_or_bindings_merge_both_exits() {
    partition("and-or-join");
}
#[test]
fn function_calls_merge_expansion_and_return_exits() {
    partition("function-exits");
}
#[test]
fn compound_use_keeps_credential_names_inert() {
    partition("use-control");
}
#[test]
fn uncertain_function_definitions_refuse() {
    partition("function-definition");
}
#[test]
fn home_expansion_uses_reachable_bindings() {
    partition("home-binding");
}
#[test]
fn original_word_regions_preserve_heredoc_context() {
    partition("word-context");
}
#[test]
fn top_level_local_joins_both_dialects() {
    partition("top-local");
}
#[test]
fn command_prefixes_are_visible_and_restored() {
    partition("prefix-scope");
}
#[test]
fn command_prefix_assignment_rhs_is_ordered() {
    partition("prefix-order");
}
#[test]
fn command_prefix_restoration_covers_early_exits() {
    partition("prefix-exits");
}
#[test]
fn local_declarations_under_prefixes_join_and_restore() {
    partition("prefix-local");
}
#[test]
fn functions_shadow_builtin_binding_and_code_sinks() {
    partition("function-shadow");
}
#[test]
fn functions_shadow_directory_builtins() {
    partition("function-shadow-directory");
}
#[test]
fn loop_exit_levels_capture_the_reachable_scope() {
    partition("loop-level");
}
#[test]
fn recursive_calls_conservatively_refuse() {
    partition("recursion");
}
#[test]
fn loop_values_keep_unsplit_dialect_candidate() {
    let row = rows()
        .into_iter()
        .find(|r| r["id"] == "loop-split-union")
        .unwrap();
    let observation = agent_guard_rust::shell::observe(
        row["source"].as_str().unwrap(),
        Arm::Brush,
        "/h",
        "/p",
        true,
    )
    .unwrap();
    let values: Vec<_> = observation
        .script
        .commands
        .iter()
        .filter(|c| c.program.is_some_and(|i| c.argv[i] == "printf"))
        .map(|c| c.argv.last().unwrap().text.as_str())
        .collect();
    for candidate in ["alpha", "beta", "alpha beta"] {
        assert!(values.contains(&candidate), "{candidate}: {values:?}");
    }
}

#[test]
fn data_use_does_not_stat_ssh_public_names() {
    use agent_guard_rust::{CheckErrorKind, filesystem, record::Effect};
    use std::{
        io,
        path::{Path, PathBuf},
    };
    struct StatFault {
        calls: usize,
    }
    impl filesystem::Probe for StatFault {
        fn read_link(&mut self, _: &Path) -> io::Result<Option<PathBuf>> {
            Ok(None)
        }
        fn stat(&mut self, _: &Path) -> io::Result<Option<filesystem::Metadata>> {
            self.calls += 1;
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
    }
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/rust-m2-use-probes.json")).unwrap();
    for effect in [Effect::Use, Effect::Read, Effect::Write, Effect::List] {
        let mut probe = StatFault { calls: 0 };
        let result = filesystem::identify_target(
            fixture["public_path"].as_str().unwrap(),
            fixture["cwd"].as_str().unwrap(),
            fixture["home"].as_str().unwrap(),
            false,
            false,
            effect,
            &mut probe,
        );
        if effect == Effect::Use {
            assert!(
                matches!(result, Ok(filesystem::Identity::Public(_))),
                "{result:?}"
            );
            assert_eq!(probe.calls, 0);
        } else {
            assert_eq!(result.unwrap_err().kind, CheckErrorKind::ProbeFault);
            assert!(probe.calls > 0);
        }
    }
}
