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
            assert!(wire.stderr.contains("call blocked") && wire.stderr.contains("recheck"));
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
