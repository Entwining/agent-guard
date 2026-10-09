#![allow(
    dead_code,
    reason = "Each test binary that includes this module uses a different subset of it."
)]

use agent_guard_rust::{
    CheckError, CheckErrorKind, Context, Coverage, CoverageGap, Disposition, Evaluation, Event,
    Outcome,
    adapters::{Consumer, render},
    evaluate_with_arm,
    filesystem::{self, Probe},
    shell::Arm,
};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

#[path = "harness/assertions.rs"]
mod assertions;
#[path = "harness/fixture.rs"]
mod fixture;
#[path = "harness/operations.rs"]
mod operations;
#[path = "harness/probe.rs"]
mod probe;
#[path = "harness/worker.rs"]
mod worker;

use assertions::{assert_effect_or_failure, assert_observers};
pub use fixture::*;
use operations::{Witness, assert_task_result, closed_operation, verify_recovery};
pub use probe::RecordingProbe;
use worker::worker_fault;

pub fn class(result: &Result<Evaluation, CheckError>) -> &'static str {
    match result {
        Err(_) => "F",
        Ok(e) => match e.outcome {
            Outcome::NoObjection => "N",
            Outcome::SoftAdvice(_) => "A",
            Outcome::ProtectedDenial { .. } => "D",
            Outcome::CoverageInsufficient {
                disposition: Disposition::ContinueLimitedPreflight,
                ..
            } => "UC",
            Outcome::CoverageInsufficient {
                disposition: Disposition::RejectUnsupportedSyntax,
                ..
            } => "UR",
            Outcome::CoverageInsufficient {
                disposition: Disposition::RequireVerifiedExecutionOwner,
                ..
            } => "UO",
        },
    }
}

fn gap_name(gap: &CoverageGap) -> String {
    match gap {
        CoverageGap::UnknownProgram { program } => format!("UnknownProgram:{program}"),
        CoverageGap::OutsideObservedTool { tool } => format!("OutsideObservedTool:{tool}"),
        other => format!("{other:?}"),
    }
}

pub fn coverage(result: &Result<Evaluation, CheckError>) -> Value {
    match result {
        Err(error) => json!({"state":"NotCompleted","error_kind":format!("{:?}",error.kind)}),
        Ok(e) => match &e.coverage {
            Coverage::SupportedPreflight => json!({"state":"SupportedPreflight"}),
            Coverage::LimitedPreflight(gaps) => {
                json!({"state":"LimitedPreflight","gaps":gaps.iter().map(gap_name).collect::<Vec<_>>()})
            }
            Coverage::OutsideObservedToolCoverage { tool } => {
                json!({"state":"OutsideObservedToolCoverage","tool_class":tool})
            }
        },
    }
}

pub struct Gate {
    pub operation_start_count: usize,
}

impl Gate {
    pub fn run(
        &mut self,
        consumer: Consumer,
        wire: &agent_guard_rust::adapters::Wire,
        operation: impl FnOnce() -> String,
    ) -> Option<String> {
        if agent_guard_rust::adapters::permits_call(consumer, wire) {
            self.operation_start_count += 1;
            Some(operation())
        } else {
            None
        }
    }
}

pub fn run(row: &Value, arm: Arm) -> Value {
    let fixture = Fixture::new();
    fixture.setup(row);
    let context = fixture.context(row);
    let body = fixture.body(row);
    let mut probe = RecordingProbe::literal_for_quoted_paths(&fixture);
    if row["fault_injection"]["operation"].as_str().is_some() {
        probe.fault = Some(fixture.project.clone());
    }
    let start = Instant::now();
    let mut lifecycle = Value::Null;
    let result = match row["fault_injection"]["owner"].as_str() {
        Some("offline checker lifecycle" | "offline checker worker") => {
            let kind = if text(row, "id").contains("deadline") {
                CheckErrorKind::Deadline
            } else if text(row, "id").contains("cancelled") {
                CheckErrorKind::Cancelled
            } else {
                CheckErrorKind::GuardFault
            };
            lifecycle = worker_fault(&fixture, kind);
            Err(CheckError { kind })
        }
        _ => evaluate_with_arm(
            Event {
                bytes: &body,
                context: &context,
                probe: &mut probe,
            },
            arm,
        ),
    };
    let wire = render(context.consumer, &result);
    let elapsed_ns = start.elapsed().as_nanos();
    let mut gate = Gate {
        operation_start_count: 0,
    };
    let mut witness = Witness::default();
    let task_result = gate.run(context.consumer, &wire, || {
        closed_operation(&fixture, row, &body, &mut witness)
    });
    if let Some(task_result) = &task_result {
        assert_task_result(&fixture, row, task_result, &witness);
    }
    let recovery = match &result {
        Ok(Evaluation {
            outcome: Outcome::ProtectedDenial { recovery, .. },
            ..
        })
        | Ok(Evaluation {
            outcome:
                Outcome::CoverageInsufficient {
                    recovery: Some(recovery),
                    ..
                },
            ..
        }) => agent_guard_rust::adapters::recovery_value(recovery),
        _ => Value::Null,
    };
    let recovery_receipt = verify_recovery(&fixture, row, &recovery, arm);
    let observed_effects = result
        .as_ref()
        .map(|evaluation| agent_guard_rust::adapters::effects_value(&evaluation.effects))
        .unwrap_or_else(|_| json!([]));
    json!({"id":row["id"],"consumer":row["consumer"],"arm":format!("{arm:?}"),"class":class(&result),"coverage":coverage(&result),"library":format!("{result:?}"),"recovery":recovery,"recovery_receipt":recovery_receipt,"exit":wire.exit,"stdout":wire.stdout,"stderr":wire.stderr,"probe_calls":probe.calls,"shell_observation_entries":context.shell_observation_entries.get(),"operation_start_count":gate.operation_start_count,"task_result":task_result,"task_witness_scope":if matches!(&result,Ok(Evaluation {coverage:Coverage::OutsideObservedToolCoverage {..},..})) || text(row,"id").contains("dynamic-chooser") {"known-public control; original task incomplete"} else {"closed fixture operation surrogate"},"observed_effects":observed_effects,"effects":witness.value(&fixture),"elapsed_ns":elapsed_ns,"lifecycle":lifecycle,"evidence_owner":if is_lifecycle_row(row) {"harness-only"} else {"evaluator"}})
}

pub fn is_lifecycle_row(row: &Value) -> bool {
    matches!(
        row["fault_injection"]["owner"].as_str(),
        Some("offline checker lifecycle" | "offline checker worker")
    )
}

pub fn is_evaluator_row(row: &Value) -> bool {
    row["consumer"] != "owned-writer" && !is_lifecycle_row(row)
}

pub fn assert_preflight_tuple(row: &Value, actual: &Value) {
    let conditional = row.get("conditional_outcome").is_some()
        && actual["observed_effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|effect| {
                effect["protection"] == "AppData"
                    && effect["write"] == false
                    && effect["source"] == "Nested"
            });
    let expected = if conditional {
        "D".to_owned()
    } else {
        row["outcome_class"].as_str().unwrap().replace('-', "")
    };
    assert_eq!(
        actual["class"].as_str().unwrap(),
        expected,
        "{}: {}",
        row["id"],
        actual["library"]
    );
    assert_eq!(
        actual["coverage"]["state"], row["expected_coverage"]["state"],
        "{} coverage",
        row["id"]
    );
    if let Some(tool) = row["expected_coverage"].get("tool_class") {
        assert_eq!(
            &actual["coverage"]["tool_class"], tool,
            "{} tool class",
            row["id"]
        );
    }
    if let Some(expected) = row["expected_coverage"]["gaps"].as_array() {
        let mut expected = expected.clone();
        expected.sort_by_key(Value::to_string);
        let mut got = actual["coverage"]["gaps"].as_array().unwrap().clone();
        got.sort_by_key(Value::to_string);
        if conditional {
            for required in expected {
                assert!(got.contains(&required), "{} conditional gap", row["id"]);
            }
        } else {
            assert_eq!(got, expected, "{} gaps", row["id"]);
        }
    }
    let class = actual["class"].as_str().unwrap();
    let expected_exit = if ["F", "D", "UR", "UO"].contains(&class) {
        2
    } else {
        0
    };
    assert_eq!(actual["exit"], expected_exit);
    if class == "A" {
        assert!(actual["stderr"].as_str().unwrap().is_empty());
    } else {
        assert!(actual["stdout"].as_str().unwrap().is_empty());
    }
    if ["N", "UC"].contains(&class) {
        assert!(actual["stderr"].as_str().unwrap().is_empty());
    }
    assert_eq!(
        !actual["stdout"].as_str().unwrap().is_empty(),
        row["advice_expectation"]["expectation"] == "present"
    );
    assert_effect_or_failure(row, actual, conditional);
}

pub fn assert_tuple(row: &Value, actual: &Value) {
    assert_preflight_tuple(row, actual);
    let class = actual["class"].as_str().unwrap();
    assert_eq!(
        actual["operation_start_count"],
        if ["N", "A", "UC"].contains(&class) {
            1
        } else {
            0
        }
    );
    if text(row, "id").starts_with("S20") && class == "F" {
        assert_eq!(actual["shell_observation_entries"], 0);
        assert_eq!(actual["probe_calls"].as_array().unwrap().len(), 0);
        assert_eq!(actual["coverage"]["error_kind"], "ResourceLimit");
    }
    assert_observers(row, actual);
}
