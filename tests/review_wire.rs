mod support;

use agent_guard_rust::{
    CheckError, CheckErrorKind,
    adapters::{Consumer, render},
};

#[test]
fn review_b1_m6_failures_block_from_consumer_wire() {
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
        ] {
            let wire = render(consumer, &Err(CheckError { kind }));
            assert_eq!(wire.exit, 2, "{consumer:?} {kind:?}");
            let sentence = "The agent guard could not complete this check, so the call is blocked. Have the checker owner repair the failed check, then recheck the call before running it.";
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
                coverage: Coverage::LimitedPreflight(vec![cause]),
                effects: Vec::new(),
            });
            let wire = render(consumer, &result);
            let sentence = "The agent guard cannot inspect this unsupported or unresolved operation. Replace the unsupported construct with a Bash-compatible command naming an explicit public path, then recheck it.";
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
