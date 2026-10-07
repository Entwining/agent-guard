use crate::support;
use agent_guard_rust::shell::{self, Arm};
use serde_json::Value;
use std::collections::BTreeSet;

fn partition(name: &str) {
    let packet: Value = serde_json::from_str(include_str!("../fixtures/rust-m2-cwd.json")).unwrap();
    let rows: Vec<_> = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["partition"] == name)
        .collect();
    assert!(!rows.is_empty(), "missing partition {name}");
    for row in rows {
        let result = shell::observe(
            row["source"].as_str().unwrap(),
            Arm::Brush,
            "/h",
            row["cwd"].as_str().unwrap(),
            true,
        )
        .unwrap();
        if let Some(gap) = row["gap"].as_str() {
            assert!(
                result.gaps.iter().any(|g| format!("{g:?}") == gap),
                "{}",
                row["id"]
            );
        }
        if let Some(gap) = row["absent_gap"].as_str() {
            assert!(
                !result.gaps.iter().any(|g| format!("{g:?}") == gap),
                "{}",
                row["id"]
            );
        }
        let commands = result
            .script
            .commands
            .iter()
            .filter(|c| {
                c.program
                    .is_some_and(|i| c.argv[i] == row["program"].as_str().unwrap())
                    && row["nested"]
                        .as_bool()
                        .is_none_or(|nested| c.nested == nested)
            })
            .collect::<Vec<_>>();
        let cwds = commands
            .iter()
            .map(|c| c.cwd.as_str())
            .collect::<BTreeSet<_>>();
        let expected = row["cwds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(cwds, expected, "{}: {result:?}", row["id"]);
        if let Some(words) = row["words"].as_array() {
            let actual = commands
                .iter()
                .flat_map(|c| &c.argv[c.program.unwrap() + 1..])
                .map(|w| w.text.as_str())
                .collect::<BTreeSet<_>>();
            assert_eq!(
                actual,
                words.iter().map(|s| s.as_str().unwrap()).collect(),
                "{}",
                row["id"]
            );
        }
        for command in commands {
            let word = &command.argv[command.program.unwrap() + 1];
            if let Some(pwd) = row["pwd"].as_bool() {
                assert_eq!(word.pwd, pwd, "{}", row["id"]);
            }
            if row["paired"] == true {
                assert_eq!(word.text, command.cwd);
                assert_eq!(word.value, command.cwd);
            }
            if let Some(prefix) = row["paired_data"].as_str() {
                assert_eq!(word.text, format!("{prefix}{}", command.cwd));
                assert_eq!(word.value, word.text);
            }
            if let Some(redirect) = row["redirect"].as_str() {
                assert_eq!(command.redirects[0].target, redirect);
            }
        }
    }
}

#[test]
fn and_success_excludes_failed_cwd() {
    partition("success");
}
#[test]
fn pushd_success_moves_cwd() {
    partition("pushd");
}
#[test]
fn no_operand_cd_reaches_home() {
    partition("no-operand");
}
#[test]
fn and_failures_survive_after_chain() {
    partition("failure");
}
#[test]
fn group_preserves_inner_cd_failure() {
    partition("compound-failure");
}
#[test]
fn function_uses_caller_directory_and_failures() {
    partition("function-failure");
}
#[test]
fn substitution_inherits_directory_and_clears_failures() {
    partition("substitution-failure");
}
#[test]
fn physical_cd_preserves_link_before_parent() {
    partition("physical");
}
#[test]
fn cd_mode_union_preserves_zsh_physical_reach() {
    partition("disputed");
}
#[test]
fn logical_tail_keeps_physical_origin() {
    partition("logical-tail");
}
#[test]
fn pwd_assignment_is_data() {
    partition("pwd-data");
}
#[test]
fn pwd_command_reads_actual_directory() {
    partition("pwd-command");
}
#[test]
fn tilde_keeps_bash_and_zsh_pwd_readings() {
    partition("tilde");
}
#[test]
fn successful_cd_resets_pwd() {
    partition("pwd-reset");
}
#[test]
fn pwd_words_follow_directory_alternatives() {
    partition("pwd-projection");
}
#[test]
fn cdpath_persistent_candidates_are_observed() {
    partition("cdpath-persistent");
}
#[test]
fn cdpath_declaration_candidates_are_observed() {
    partition("cdpath-declaration");
}
#[test]
fn cdpath_prefix_candidates_are_observed() {
    partition("cdpath-local");
}
#[test]
fn cdpath_does_not_override_dot_absolute_or_home() {
    partition("cdpath-controls");
}
#[test]
fn runtime_cdpath_has_no_scope_refusal() {
    partition("cdpath-unknown");
}
#[test]
fn cdpath_protected_candidates_are_observed() {
    partition("cdpath-protected");
}
#[test]
fn assigned_pwd_protection_uses_data_and_actual_directory() {
    partition("pwd-protected");
}
#[test]
fn assigned_pwd_benign_operand_is_not_reprojected() {
    partition("pwd-control");
}
#[test]
fn tilde_union_retains_protected_assignment() {
    partition("pwd-tilde-protected");
}
#[test]
fn plain_pwd_projection_keeps_actual_directory() {
    partition("pwd-variable-projection");
}
#[test]
fn tilde_projection_keeps_actual_directory() {
    partition("pwd-tilde-projection");
}
#[test]
fn split_cwd_origin_is_conservatively_unresolved() {
    partition("cwd-split");
}
#[test]
fn conditional_clears_directory_failures() {
    partition("conditional-collector");
}
#[test]
fn loop_clears_directory_failures() {
    partition("loop-collector");
}

#[test]
fn command_alternatives_union_failure_collectors() {
    partition("alternative-collectors");
}

#[test]
fn declaration_cwd_origin_keeps_assignment_prefix() {
    partition("declaration-projection");
}

#[test]
fn split_cwd_origin_tracks_each_word() {
    partition("split-range-projection");
}

#[test]
fn decided_pwd_and_cdpath_wire_contracts() {
    use agent_guard_rust::{Event, adapters, evaluate_with_arm};
    use serde_json::json;
    let packet: Value = serde_json::from_str(include_str!("../fixtures/rust-m2-cwd.json")).unwrap();
    let fixture = support::Fixture::new();
    for row in packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["expected"].is_string())
    {
        let cwd = row["cwd"]
            .as_str()
            .unwrap()
            .replacen("/h", &fixture.home, 1);
        for consumer in ["claude", "codex", "pi"] {
            let context = fixture.context(&json!({"consumer":consumer,"cwd":cwd}));
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
                "{}: {consumer}: {result:?}",
                row["id"]
            );
            let wire = adapters::render(context.consumer, &result);
            assert_eq!(wire.exit, if row["expected"] == "N" { 0 } else { 2 });
            assert!(wire.stdout.is_empty());
            if row["expected"] == "N" {
                assert!(wire.stderr.is_empty());
                assert_eq!(support::coverage(&result)["state"], "SupportedPreflight");
            } else {
                assert!(wire.stderr.contains("app-data"));
            }
        }
    }
}
