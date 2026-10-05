mod support;
use agent_guard_rust::{Event, adapters, evaluate_with_arm, shell::Arm};
use serde_json::{Value, json};

fn rows() -> Vec<Value> {
    [
        include_str!("fixtures/rust-m2-1.json"),
        include_str!("fixtures/rust-m2-1-armed.json"),
        include_str!("fixtures/rust-batch1.json"),
        include_str!("fixtures/rust-batch1-joins.json"),
    ]
    .into_iter()
    .flat_map(|source| {
        let packet = serde_json::from_str::<Value>(source).unwrap();
        packet["rows"]
            .as_array()
            .unwrap()
            .iter()
            .cloned()
            .map(|mut row| {
                if let Some(count) = row["failed_moves"].as_u64() {
                    let mut source = packet["overflow"]["initial"].as_str().unwrap().to_owned();
                    for n in 0..count {
                        source.push_str(
                            &packet["overflow"]["step"]
                                .as_str()
                                .unwrap()
                                .replace("$N", &n.to_string()),
                        );
                    }
                    source.push_str(row["source"].as_str().unwrap());
                    row["source"] = source.into();
                }
                row
            })
            .collect::<Vec<_>>()
    })
    .collect()
}

fn partition(name: &str) {
    let fixture = support::Fixture::new();
    let rows: Vec<_> = rows()
        .into_iter()
        .filter(|r| r["partition"] == name)
        .collect();
    assert!(!rows.is_empty(), "missing partition {name}");
    for row in rows {
        for consumer in ["claude", "codex", "pi"] {
            let mut context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
            if let Some(cwd) = row["cwd"].as_str() {
                context.cwd = cwd.replace("/h", &fixture.home);
            }
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
                "{consumer}: {}: {result:?}",
                row["id"]
            );
            let wire = adapters::render(context.consumer, &result);
            let permitted = matches!(row["expected"].as_str().unwrap(), "N" | "UC" | "A");
            assert_eq!(wire.exit, if permitted { 0 } else { 2 });
            assert!(wire.stdout.is_empty());
            if let Some(reason) = row["reason"].as_str() {
                assert!(
                    wire.stderr.contains(reason),
                    "{}: {}",
                    row["id"],
                    wire.stderr
                );
            }
            if row["expected"] == "N" {
                assert!(wire.stderr.is_empty());
                assert_eq!(support::coverage(&result)["state"], "SupportedPreflight");
            }
            if let Some(gap) = row["gap"].as_str() {
                assert!(
                    support::coverage(&result)["gaps"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|g| g == gap)
                );
            }
        }
    }
}

#[test]
fn runtime_bindings_preserve_known_path_text() {
    partition("batch1-runtime-text");
}
#[test]
fn cwd_overflow_reaches_nested_code() {
    partition("batch1-nested-cwd");
}
#[test]
fn absent_join_candidates_keep_baseline_coverage() {
    partition("batch1-join");
}
#[test]
fn loop_state_converges_after_oldpwd_catches_up() {
    partition("batch1-loop-convergence");
}
#[test]
fn directory_convergence_is_independent_of_oldpwd() {
    partition("batch1-directory-comparison");
}
#[test]
fn tree_roots_list() {
    partition("tree");
}
#[test]
fn finite_loop_directories_keep_protected_candidates() {
    partition("loop-cwd");
}
#[test]
fn oldpwd_bindings_move_the_tracked_directory() {
    partition("cwd-oldpwd");
}
#[test]
fn two_operand_cd_retains_both_shell_readings() {
    partition("cwd-two-operands");
}
#[test]
fn visible_file_roots_list() {
    partition("files-visible");
}
#[test]
fn hidden_file_roots_read() {
    partition("files-hidden");
}

#[test]
fn or_rhs_uses_left_exit_bindings() {
    partition("or-bindings");
}
#[test]
fn or_rhs_uses_left_exit_directory() {
    partition("or-directory");
}
#[test]
fn cwd_candidate_loss_refuses_relative_targets() {
    partition("cwd-budget");
}
#[test]
fn cwd_budget_survives_branch_join() {
    partition("cwd-budget-join");
}
#[test]
fn runtime_values_keep_unresolved_target_contract() {
    partition("runtime-target");
}
#[test]
fn runtime_cdpath_keeps_the_baseline_class() {
    partition("runtime-cdpath");
}
#[test]
fn runtime_read_does_not_refuse_syntax() {
    partition("runtime-read");
}
#[test]
fn runtime_math_values_keep_pre_m2_class() {
    partition("runtime-math");
}
#[test]
fn runtime_cd_keeps_existing_uncertainty() {
    partition("runtime-cd");
}
#[test]
fn runtime_arithmetic_keeps_pre_m2_class() {
    partition("runtime-arithmetic");
}
#[test]
fn armed_values_propagate_through_expansion() {
    partition("armed-expansion");
}
#[test]
fn function_arguments_bind_armed_positionals() {
    partition("armed-function");
}
#[test]
fn set_arguments_bind_armed_positionals() {
    partition("armed-positional");
}
#[test]
fn printf_assigns_literal_armed_values() {
    partition("armed-printf");
}
#[test]
fn append_combines_values_before_arming() {
    partition("armed-append");
}
#[test]
fn indexed_values_reach_arithmetic_references() {
    partition("armed-indexed");
}
#[test]
fn compound_array_values_reach_arithmetic_references() {
    partition("armed-array");
}
#[test]
fn literal_read_values_reach_arithmetic_references() {
    partition("armed-read");
}
#[test]
fn unmodeled_armed_printf_refuses() {
    partition("armed-printf-fallback");
}
#[test]
fn unmodeled_armed_read_refuses() {
    partition("armed-read-fallback");
}
#[test]
fn positional_values_reach_target_operands() {
    partition("armed-positional-value");
}
#[test]
fn indexed_values_reach_target_operands() {
    partition("armed-indexed-value");
}
#[test]
fn dynamic_array_indices_keep_their_variable_names() {
    partition("indexed-vars");
    let row = rows()
        .into_iter()
        .find(|row| row["id"] == "dynamic-index-control")
        .unwrap();
    let result = agent_guard_rust::shell::observe(
        row["source"].as_str().unwrap(),
        Arm::Brush,
        "/h",
        "/h/project",
        true,
    )
    .unwrap();
    let word = result
        .script
        .commands
        .iter()
        .rev()
        .find(|command| {
            command
                .argv
                .first()
                .is_some_and(|word| word.text == "printf")
        })
        .unwrap()
        .argv
        .last()
        .unwrap();
    assert_eq!(word.vars, ["arr", "INDEX"]);
}
#[test]
fn array_locals_restore_indexed_binding_state() {
    partition("armed-array-scope");
}
#[test]
fn local_array_elements_do_not_escape_return() {
    partition("armed-array-local");
}
#[test]
fn append_candidate_product_is_bounded() {
    partition("armed-append-budget");
}
#[test]
fn positional_and_indexed_values_materialize() {
    for id in [
        "unarmed-positional-value",
        "unarmed-array-value",
        "unarmed-function-control",
    ] {
        let row = rows().into_iter().find(|row| row["id"] == id).unwrap();
        let result = agent_guard_rust::shell::observe(
            row["source"].as_str().unwrap(),
            Arm::Brush,
            "/h",
            "/h/project",
            true,
        )
        .unwrap();
        let command = result
            .script
            .commands
            .iter()
            .rfind(|command| {
                command
                    .program
                    .is_some_and(|index| command.argv[index] == "printf")
            })
            .unwrap();
        assert_eq!(
            command.argv.last().unwrap().text,
            "public",
            "{id}: {result:?}"
        );
    }
}
