use agent_guard_rust::{
    CheckError, Disposition, Evaluation, Event, Outcome, adapters, evaluate_with_arm, shell::Arm,
};
use serde_json::{Value, json};
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

struct NoIoProbe;
impl agent_guard_rust::filesystem::Probe for NoIoProbe {
    fn stat(
        &mut self,
        _: &std::path::Path,
    ) -> std::io::Result<Option<agent_guard_rust::filesystem::Metadata>> {
        Ok(None)
    }
    fn read_link(&mut self, _: &std::path::Path) -> std::io::Result<Option<std::path::PathBuf>> {
        Ok(None)
    }
}

fn main() {
    let rows: Vec<Value> = include_str!("../tests/fixtures/rust-parser-inputs.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for consumer in ["claude", "codex", "pi"] {
        let context = agent_guard_rust::Context {
            consumer: match consumer {
                "claude" => adapters::Consumer::Claude,
                "codex" => adapters::Consumer::Codex,
                _ => adapters::Consumer::Pi,
            },
            home: "/synthetic/home".into(),
            cwd: "/synthetic/home/project".into(),
            user: Some("fixture-user".into()),
            zsh_executor: consumer != "pi",
            require_execution_owner: false,
            shell_observation_entries: std::cell::Cell::new(0),
        };
        for row in &rows {
            let mut event = if let Some(source) = row["source"].as_str() {
                json!({"tool_name":if consumer=="pi" {"bash"} else {"Bash"},"tool_input":{"command":source}})
            } else {
                row["event"].clone()
            };
            if consumer == "pi" && event["tool_name"] == "Bash" {
                event["tool_name"] = json!("bash");
            }
            let bytes = serde_json::to_vec(&event).unwrap();
            let result = evaluate_with_arm(
                Event {
                    bytes: &bytes,
                    context: &context,
                    probe: &mut NoIoProbe,
                },
                Arm::Brush,
            );
            let wire = adapters::render(context.consumer, &result);
            println!(
                "{}",
                json!({"manifest_observation":true,"id":row["id"],"consumer":consumer,"class":class(&result),"evaluation":format!("{result:?}"),"exit":wire.exit,"stdout":wire.stdout,"stderr":wire.stderr})
            );
        }
    }
}
