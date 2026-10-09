use crate::support;

use agent_guard_rust::{
    CheckError, CheckErrorKind,
    adapters::{Consumer, render},
};

#[test]
fn check_failures_block_each_consumer() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/rust-refusal-advice.json")).unwrap();
    for consumer in [Consumer::Claude, Consumer::Codex, Consumer::Pi] {
        for kind in [
            CheckErrorKind::MalformedInput,
            CheckErrorKind::InputFailure,
            CheckErrorKind::GuardFault,
            CheckErrorKind::ProbeFault,
            CheckErrorKind::ResourceLimit,
            CheckErrorKind::Deadline,
            CheckErrorKind::Cancelled,
            CheckErrorKind::BrokenEnrollment,
            CheckErrorKind::RelativeCwd,
            CheckErrorKind::InvalidEncoding,
        ] {
            let wire = render(consumer, &Err(CheckError { kind }));
            assert_eq!(wire.exit, 2, "{consumer:?} {kind:?}");
            let row = fixture["errors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["kind"] == format!("{kind:?}"))
                .unwrap();
            let sentence = row["sentence"].as_str().unwrap();
            let expected = if consumer == Consumer::Claude {
                format!(
                    "DENIED: {sentence} Do NOT bypass this restriction or retry the same blocked command.\n"
                )
            } else {
                format!("{sentence}\n")
            };
            assert_eq!(wire.stderr, expected, "{consumer:?} {kind:?}");
            assert!(wire.stdout.is_empty());
            let mut gate = support::Gate {
                operation_start_count: 0,
            };
            assert!(
                gate.run(consumer, &wire, || "forbidden operation".into())
                    .is_none()
            );
            assert_eq!(gate.operation_start_count, 0);
        }
    }
}

#[test]
fn unresolved_operations_offer_an_explicit_public_path() {
    use agent_guard_rust::{Coverage, CoverageGap, Disposition, Evaluation, Outcome};
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/rust-refusal-advice.json")).unwrap();
    for consumer in [Consumer::Claude, Consumer::Codex, Consumer::Pi] {
        for cause in [
            CoverageGap::IdentityBound,
            CoverageGap::InspectionBudget,
            CoverageGap::ExecutorDivergence,
            CoverageGap::UnsupportedDialectConstruct,
        ] {
            let result = Ok(Evaluation {
                outcome: Outcome::CoverageInsufficient {
                    cause: cause.clone(),
                    disposition: Disposition::RejectUnsupportedSyntax,
                    recovery: None,
                },
                coverage: Coverage::LimitedPreflight(vec![cause.clone()]),
                effects: Vec::new(),
            });
            let wire = render(consumer, &result);
            let row = fixture["refusals"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["kind"] == format!("{cause:?}"))
                .unwrap();
            let sentence = row["sentence"].as_str().unwrap();
            let expected = if consumer == Consumer::Claude {
                format!(
                    "DENIED: {sentence} Do NOT bypass this restriction or retry the same blocked command.\n"
                )
            } else {
                format!("{sentence}\n")
            };
            assert_eq!(wire.exit, 2);
            assert_eq!(wire.stderr, expected);
            assert!(wire.stdout.is_empty());
        }
    }
}
