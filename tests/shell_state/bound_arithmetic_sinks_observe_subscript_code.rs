use crate::support;
use agent_guard_rust::{
    CoverageGap, Event, adapters, evaluate_with_arm,
    shell::{self, Arm},
};
use serde_json::{Value, json};

fn rows() -> Vec<Value> {
    serde_json::from_str(include_str!("../fixtures/rust-m2-arithmetic.json")).unwrap()
}

fn bodies(source: &str) -> Vec<String> {
    shell::observe(
        source,
        Arm::Brush,
        "/synthetic/home",
        "/synthetic/home/project",
        true,
    )
    .unwrap()
    .script
    .commands
    .into_iter()
    .filter(|c| c.nested)
    .filter_map(|c| c.argv.first().map(|w| w.text.clone()))
    .collect()
}

#[test]
fn bound_arithmetic_sinks_observe_subscript_code() {
    let rows: Vec<_> = rows()
        .into_iter()
        .filter(|r| r["sink"].is_string())
        .collect();
    assert!(!rows.is_empty(), "missing arithmetic sink partition");
    for row in rows {
        assert!(
            bodies(row["source"].as_str().unwrap()).contains(&"cat".into()),
            "arithmetic sink {}: {}",
            row["id"],
            row["scope"]
        );
    }
}

#[test]
fn arithmetic_effects_and_consumer_wire() {
    let fixture = support::Fixture::new();
    let rows = rows();
    assert!(!rows.is_empty(), "missing arithmetic consumer partition");
    for consumer in ["claude", "codex", "pi"] {
        let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
        for row in &rows {
            let bytes = serde_json::to_vec(&json!({"tool_name":if consumer == "pi" {"bash"} else {"Bash"},"tool_input":{"command":row["source"]}})).unwrap();
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
                "{consumer}: {row}: {result:?}"
            );
            let wire = adapters::render(context.consumer, &result);
            assert_eq!(wire.exit, if row["expected"] == "N" { 0 } else { 2 });
            assert!(wire.stdout.is_empty());
            assert!(!wire.stderr.contains("checker failed"));
            if row["expected"] == "D" {
                assert!(wire.stderr.contains("credential or environment file"));
            }
        }
    }
}

#[test]
fn arithmetic_uses_current_binding_version_and_scope() {
    let rows: Vec<_> = rows()
        .into_iter()
        .filter(|r| !r["sink"].is_string() && r["id"] != "cycle")
        .collect();
    assert!(
        !rows.is_empty(),
        "missing arithmetic binding scope partition"
    );
    for row in rows {
        assert_eq!(
            bodies(row["source"].as_str().unwrap()).contains(&"cat".into()),
            row["nested"].as_bool().unwrap(),
            "{}: {}",
            row["id"],
            row["scope"]
        );
    }
}

#[test]
fn arithmetic_binding_cycles_are_bounded() {
    let rows = rows();
    let row = rows.iter().find(|r| r["id"] == "cycle").unwrap();
    let o = shell::observe(
        row["source"].as_str().unwrap(),
        Arm::Brush,
        "/h",
        "/h/p",
        true,
    )
    .unwrap();
    assert!(
        o.gaps.contains(&CoverageGap::InspectionBudget),
        "recursive binding cannot imply complete coverage"
    );
}

#[test]
fn arithmetic_binding_work_is_bounded() {
    let rows = rows();
    let row = rows.iter().find(|r| r["id"] == "fanout").unwrap();
    let observation = shell::observe(
        row["source"].as_str().unwrap(),
        Arm::Brush,
        "/h",
        "/h/p",
        true,
    )
    .unwrap();
    assert!(
        observation.gaps.contains(&CoverageGap::InspectionBudget),
        "acyclic binding fanout must exhaust the finite work budget"
    );
}
