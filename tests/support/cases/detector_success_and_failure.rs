pub(crate) use crate::support;
pub use agent_guard_rust::{
    CheckErrorKind, Context, Coverage, CoverageGap, Disposition, Evaluation, Event, Outcome,
    adapters::{Consumer, render},
    evaluate_with_arm,
    shell::{self, Arm},
};
pub use serde_json::{Value, json};
pub use std::cell::Cell;

pub fn context(fixture: &support::Fixture) -> Context {
    fixture.context(
        &support::rows()
            .into_iter()
            .find(|row| row["id"] == "S01-read-public-claude")
            .unwrap(),
    )
}
pub fn check(
    fixture: &support::Fixture,
    ctx: &Context,
    probe: &mut support::RecordingProbe,
    arm: Arm,
    source: &str,
) -> Result<Evaluation, agent_guard_rust::CheckError> {
    let body = serde_json::to_vec(&json!({"tool_name":if ctx.consumer==Consumer::Pi {"bash"} else {"Bash"},"tool_input":{"command":source}})).unwrap();
    let result = evaluate_with_arm(
        Event {
            bytes: &body,
            context: ctx,
            probe,
        },
        arm,
    );
    assert!(
        probe
            .calls
            .iter()
            .all(|path| !path.contains("/Containers/"))
    );
    let _ = fixture;
    result
}
