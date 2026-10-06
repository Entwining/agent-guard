use agent_guard_rust::{
    CheckErrorKind, Context, Event,
    adapters::{self, Consumer},
    evaluate_with_deadline,
    filesystem::DiskProbe,
};
use std::{cell::Cell, time::Instant};

fn check(deadline: Instant) -> Result<agent_guard_rust::Evaluation, agent_guard_rust::CheckError> {
    let context = Context {
        consumer: Consumer::Claude,
        home: "/synthetic-home".into(),
        cwd: "/synthetic-project".into(),
        user: None,
        zsh_executor: true,
        require_execution_owner: false,
        shell_observation_entries: Cell::new(0),
    };
    evaluate_with_deadline(
        Event {
            bytes: br#"{"tool_name":"Bash","tool_input":{"command":"printf public; true"}}"#,
            context: &context,
            probe: &mut DiskProbe,
        },
        deadline,
    )
}

#[test]
fn expired_evaluation_deadline_blocks_every_consumer() {
    let result = check(Instant::now());
    assert_eq!(result.as_ref().unwrap_err().kind, CheckErrorKind::Deadline);
    for consumer in [Consumer::Claude, Consumer::Codex, Consumer::Pi] {
        let wire = adapters::render(consumer, &result);
        assert_eq!(wire.exit, 2);
        assert!(wire.stdout.is_empty());
        assert!(wire.stderr.contains("checker deadline exceeded"));
    }
}
