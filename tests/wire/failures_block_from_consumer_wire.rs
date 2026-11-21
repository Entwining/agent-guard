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
            let marker = format!("[error:agent-guard/{}]", kind.id());
            let expected = if consumer == Consumer::Claude {
                format!("{marker} DENIED: {sentence}\n")
            } else {
                format!("{marker} {sentence}\n")
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
