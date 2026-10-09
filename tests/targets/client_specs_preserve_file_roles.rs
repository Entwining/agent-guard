use crate::support;
use agent_guard_rust::{Event, adapters, evaluate_with_arm, shell::Arm};
use serde_json::{Value, json};

fn partition(name: &str) {
    let fixture = support::Fixture::new();
    let packet: Value = serde_json::from_str(include_str!("../fixtures/rust-batch4.json")).unwrap();
    let setup: Value = serde_json::from_str(include_str!("../fixtures/filesystem.json")).unwrap();
    let rows: Vec<_> = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["partition"] == name)
        .collect();
    assert!(!rows.is_empty(), "missing client role partition {name}");
    for row in rows {
        for consumer in ["claude", "codex", "pi"] {
            let context = fixture.context(&json!({"consumer":consumer,"cwd":row["cwd"]}));
            let bytes = serde_json::to_vec(&json!({"tool_name":if consumer=="pi" {"bash"} else {"Bash"},"tool_input":{"command":fixture.expand(row["source"].as_str().unwrap())}})).unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            for pair in setup["links"].as_array().unwrap() {
                probe.links.insert(
                    format!("{}/{}", fixture.home, pair[0].as_str().unwrap()),
                    fixture.expand(pair[1].as_str().unwrap()),
                );
            }
            let result = evaluate_with_arm(
                Event {
                    bytes: &bytes,
                    context: &context,
                    probe: &mut probe,
                },
                Arm::Brush,
            );
            assert_eq!(
                support::class(&result),
                row["expected"],
                "{consumer}: {}: {result:?}",
                row["id"]
            );
            let wire = adapters::render(context.consumer, &result);
            assert_eq!(wire.exit, if row["expected"] == "D" { 2 } else { 0 });
            assert!(wire.stdout.is_empty());
            if row["expected"] == "D" {
                assert!(!wire.stderr.is_empty());
                assert!(!result.as_ref().unwrap().effects.is_empty());
                if let Some(kind) = row["effect"].as_str() {
                    let effect = match kind {
                        "HostingToken" => agent_guard_rust::EffectRecord::HostingToken,
                        "Keychain" => agent_guard_rust::EffectRecord::Keychain,
                        "StoredSecret" => agent_guard_rust::EffectRecord::StoredSecret,
                        "NetworkTrace" => agent_guard_rust::EffectRecord::NetworkTrace,
                        _ => panic!("unknown effect"),
                    };
                    assert!(
                        result.as_ref().unwrap().effects.contains(&effect),
                        "{}",
                        row["id"]
                    );
                    assert!(
                        wire.stderr
                            .contains(row["reason_contains"].as_str().unwrap()),
                        "{}: {}",
                        row["id"],
                        wire.stderr
                    );
                }
            } else if row["expected"] == "N" {
                assert!(wire.stderr.is_empty());
            }
            assert!(!matches!(
                result.as_ref().unwrap().outcome,
                agent_guard_rust::Outcome::SoftAdvice(_)
            ));
        }
    }
}

#[test]
fn client_specs_preserve_file_roles() {
    partition("spec");
}

#[test]
fn curl_file_inputs_and_outputs_follow_go() {
    partition("curl");
}

#[test]
fn wget_file_inputs_and_outputs_follow_go() {
    partition("wget");
}

#[test]
fn docker_host_inputs_respect_command_boundaries() {
    partition("docker");
}

#[test]
fn stored_secret_outputs_follow_the_secret_owner() {
    partition("secret");
}

#[test]
fn curl_trace_detection_skips_option_values() {
    partition("trace");
}
