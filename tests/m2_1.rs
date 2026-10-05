mod support;
use agent_guard_rust::{Event, adapters, evaluate_with_arm, shell::Arm};
use serde_json::{Value, json};

fn rows() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("fixtures/rust-m2-1.json")).unwrap()["rows"]
        .as_array()
        .unwrap()
        .clone()
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
fn tree_roots_list() {
    partition("tree");
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
