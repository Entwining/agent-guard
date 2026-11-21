use crate::support;
use agent_guard_rust::{Event, adapters, evaluate};
use serde_json::{Value, json};

pub fn partition(name: &str, packet: &str) {
    partition_with(name, packet, |_, _| {});
}

pub fn partition_with(
    name: &str,
    packet: &str,
    assertion: impl Fn(&Value, &agent_guard_rust::Evaluation),
) {
    let packet: Value = serde_json::from_str(packet).unwrap();
    let rows: Vec<_> = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["partition"] == name)
        .collect();
    assert!(
        !rows.is_empty(),
        "missing program contract partition {name}"
    );
    for row in rows {
        let fixture = support::Fixture::new();
        fixture.setup(row);
        for consumer in ["claude", "codex", "pi"] {
            let cwd = row["cwd"].as_str().unwrap_or("$P");
            let context = fixture
                .context(&json!({"consumer":consumer,"cwd":cwd,"config_dirs":row["config_dirs"]}));
            let tool =
                row["tool"]
                    .as_str()
                    .unwrap_or(if consumer == "pi" { "bash" } else { "Bash" });
            let bytes = serde_json::to_vec(
                &json!({"tool_name":tool,"tool_input":fixture.expand_value(&row["input"])}),
            )
            .unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate(Event {
                bytes: &bytes,
                context: &context,
                probe: &mut probe,
            });
            assert_eq!(
                support::class(&result),
                row["expected"],
                "{consumer}: {}: {result:?}",
                row["id"]
            );
            let wire = adapters::render(context.consumer, &result);
            assert_eq!(
                wire.exit,
                if row["expected"] == "D" || row["expected"] == "F" {
                    2
                } else {
                    0
                },
                "{consumer}: {}",
                row["id"]
            );
            assert_eq!(
                wire.stdout.is_empty(),
                row["expected"] != "A" || consumer != "claude",
                "{consumer}: {}",
                row["id"]
            );
            assert_eq!(
                wire.stderr.is_empty(),
                row["expected"] != "D" && row["expected"] != "F",
                "{consumer}: {}",
                row["id"]
            );
            if let Some(reason) = row["wire_reason"].as_str() {
                let diagnostic = match &result {
                    Ok(e) => match &e.outcome {
                        agent_guard_rust::Outcome::ProtectedDenial { reason, .. } => {
                            reason.effect.clone()
                        }
                        _ => wire.stderr.clone(),
                    },
                    Err(error) => error.to_string(),
                };
                assert!(
                    diagnostic.contains(reason),
                    "{consumer}: {}: {}",
                    row["id"],
                    wire.stderr
                );
                assert!(
                    !wire.stderr.contains("recovery:"),
                    "{consumer}: {}",
                    row["id"]
                );
            }
            if let Some(text) = row["wire_text"].as_str() {
                assert!(
                    wire.stderr.contains(&fixture.expand(text)),
                    "{consumer}: {}: {}",
                    row["id"],
                    wire.stderr
                );
            }
            if let Ok(evaluation) = &result {
                assertion(row, evaluation);
            }
        }
    }
}
