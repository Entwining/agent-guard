use crate::support;
use agent_guard_rust::{
    Context, Event,
    adapters::{Consumer, render},
    evaluate,
    filesystem::DiskProbe,
};
use serde_json::{Value, json};
use std::cell::Cell;

#[test]
fn native_legacy_client_roles_match_go_without_losing_protection() {
    let fixture = support::Fixture::new();
    let rows: Vec<_> = include_str!("../fixtures/rust-b11-entry-roles.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect();
    assert!(!rows.is_empty(), "missing native client-role partition");
    for row in rows {
        for consumer in [Consumer::Claude, Consumer::Codex, Consumer::Pi] {
            let context = Context {
                consumer,
                home: fixture.home.clone(),
                user: Some("fixture-user".into()),
                cwd: fixture.project.clone(),
                zsh_executor: consumer != Consumer::Pi,
                require_execution_owner: false,
                shell_observation_entries: Cell::new(0),
            };
            let bytes = serde_json::to_vec(&json!({"tool_name":"Bash", "tool_input":{"command":fixture.expand(row["command"].as_str().unwrap())}})).unwrap();
            let result = evaluate(Event {
                bytes: &bytes,
                context: &context,
                probe: &mut DiskProbe,
            });
            assert_eq!(
                support::class(&result),
                row["class"].as_str().unwrap(),
                "{} {consumer:?}",
                row["id"]
            );
            assert_eq!(
                render(consumer, &result).exit,
                if row["class"] == "N" { 0 } else { 2 }
            );
        }
    }
}
