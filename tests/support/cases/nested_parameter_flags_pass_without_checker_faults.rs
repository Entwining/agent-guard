pub(crate) use crate::support;

pub use agent_guard_rust::{Coverage, CoverageGap, Event, adapters, evaluate};
pub use serde_json::{Value, json};

pub fn check_rows() {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("../../fixtures/rust-m1-4-fragments.json")).unwrap();
    let fixture = support::Fixture::new();
    let mut failures = Vec::new();
    for consumer in ["claude", "codex", "pi"] {
        let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
        for row in &rows {
            let body = serde_json::to_vec(&json!({
                "tool_name":if consumer == "pi" {"bash"} else {"Bash"},
                "tool_input":{"command":row["source"]}
            }))
            .unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate(Event {
                bytes: &body,
                context: &context,
                probe: &mut probe,
            });
            let wire = adapters::render(context.consumer, &result);
            let class = support::class(&result);
            if class != row["expected"] {
                failures.push(json!({"id":row["id"],"consumer":consumer,"expected":row["expected"],"class":class,"result":format!("{result:?}"),"exit":wire.exit,"stderr":wire.stderr}));
                continue;
            }
            assert!(wire.stdout.is_empty());
            assert_eq!(wire.exit, if class == "N" { 0 } else { 2 });
        }
    }
    println!("{}", json!({"failures":failures}));
    assert!(
        failures.is_empty(),
        "{} regression failures",
        failures.len()
    );
}

pub fn mechanism_rows(owner: &str) {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("../../fixtures/rust-m1-4-regressions.json")).unwrap();
    let fixture = support::Fixture::new();
    let mut failures = Vec::new();
    for consumer in ["claude", "codex", "pi"] {
        let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
        let selected: Vec<_> = rows.iter().filter(|r| r["owner"] == owner).collect();
        assert!(!selected.is_empty(), "missing owner {owner}");
        for row in selected {
            let source = fixture.expand(row["source"].as_str().unwrap());
            let bytes = serde_json::to_vec(&json!({"tool_name":if consumer == "pi" {"bash"} else {"Bash"},"tool_input":{"command":source}})).unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate(Event {
                bytes: &bytes,
                context: &context,
                probe: &mut probe,
            });
            let wire = adapters::render(context.consumer, &result);
            let expected = row["expected"].as_str().unwrap();
            if support::class(&result) != expected
                || wire.exit != if matches!(expected, "N" | "UC") { 0 } else { 2 }
            {
                failures.push(json!({"id":row["id"],"consumer":consumer,"expected":row["expected"],"actual":support::class(&result),"result":format!("{result:?}"),"wire":format!("{wire:?}")}));
            }
            assert!(wire.stdout.is_empty());
            assert_mechanism_outcome(row, &result, &wire);
        }
    }
    println!("{}", json!({"owner":owner,"failures":failures}));
    assert!(
        failures.is_empty(),
        "{owner} observation failures: {}",
        failures.len()
    );
}

fn assert_mechanism_outcome(
    row: &Value,
    result: &Result<agent_guard_rust::Evaluation, agent_guard_rust::CheckError>,
    wire: &agent_guard_rust::adapters::Wire,
) {
    match &result.as_ref().unwrap().outcome {
        agent_guard_rust::Outcome::NoObjection => assert!(wire.stderr.is_empty()),
        agent_guard_rust::Outcome::ProtectedDenial { reason, recovery } => {
            let environmental = matches!(
                row["id"].as_str().unwrap(),
                "A102-herestring-eval"
                    | "A043-herestring-direct"
                    | "A056-subscript-assign"
                    | "g1-plain-unquoted"
                    | "g8-plain-literal"
                    | "coverage-sibling"
                    | "coverage-nested-effect"
                    | "coverage-eval-queue"
                    | "coverage-qualifier-queue"
                    | "qualifier-nested-quotes"
                    | "i4-arith-sub-plain"
                    | "i5-arith-sub-param"
            );
            assert!(
                reason.effect.contains(if environmental {
                    "environment"
                } else {
                    "private-key"
                }),
                "{row}: {reason:?}"
            );
            assert!(wire.stderr.contains(reason.rule.message()));
            assert!(!recovery.excluded_scope.is_empty());
            assert!(!recovery.automatic_application_supported);
            assert!(!wire.stderr.contains("recovery:"));
        }
        other => panic!("unexpected advice/outcome: {row}: {other:?}"),
    }
}
