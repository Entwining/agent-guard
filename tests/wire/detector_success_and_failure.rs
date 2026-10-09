#[path = "../support/cases/detector_success_and_failure.rs"]
pub mod cases;
use cases::*;

#[test]
fn gate_a_zero_starts_and_bypass_negative() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
    let result = check(
        &fixture,
        &ctx,
        &mut probe,
        Arm::Brush,
        &format!("cat '{}/data.txt'", fixture.container),
    );
    assert_eq!(support::class(&result), "D");
    let read_count = Cell::new(0);
    let operation = || {
        read_count.set(read_count.get() + 1);
        std::fs::read_to_string(format!("{}/data.txt", fixture.container)).unwrap()
    };
    let mut gate = support::Gate {
        operation_start_count: 0,
    };
    let wire = agent_guard_rust::adapters::render(ctx.consumer, &result);
    assert!(gate.run(ctx.consumer, &wire, operation).is_none());
    assert_eq!(gate.operation_start_count, 0);
    assert_eq!(read_count.get(), 0);
    let mut bypass = support::Gate {
        operation_start_count: 0,
    };
    let mut broken_wire = wire;
    broken_wire.exit = 0;
    assert!(bypass.run(ctx.consumer, &broken_wire, operation).is_some());
    assert_eq!(read_count.get(), 1);
    assert_ne!(bypass.operation_start_count, 0);
}

#[test]
fn adapters_raw_and_normalized_identity() {
    use agent_guard_rust::adapters::decode;
    for (consumer, tool, expected) in [
        (Consumer::Codex, "exec_command", "/workdir"),
        (Consumer::Codex, "Bash", "/envelope"),
        (Consumer::Claude, "exec_command", "/envelope"),
    ] {
        let bytes = serde_json::to_vec(&json!({"tool_name":tool, "cwd":"/envelope", "tool_input":{"command":"true", "cmd":"true", "workdir":"/workdir"}})).unwrap();
        assert_eq!(decode(consumer, &bytes, "/fallback").unwrap().cwd, expected);
    }
    let cases: [(Consumer, Value, Value); 3] = [
        (
            Consumer::Claude,
            json!({"tool_name":"Read","tool_input":{"file_path":"/project/input.txt"}}),
            json!({"tool_name":"Read","tool_input":{"file_path":"/project/input.txt"}}),
        ),
        (
            Consumer::Codex,
            json!({"tool_name":"Bash","tool_input":{"command":"cat /project/input.txt"}}),
            json!({"tool_name":"exec_command","arguments":"{\"cmd\":\"cat /project/input.txt\"}"}),
        ),
        (
            Consumer::Pi,
            json!({"tool_name":"read","tool_input":{"path":"/project/input.txt"}}),
            json!({"type":"tool_call","toolName":"read","input":{"path":"/project/input.txt"}}),
        ),
    ];
    for (consumer, normalized, raw) in cases {
        let normalized = decode(
            consumer,
            &serde_json::to_vec(&normalized).unwrap(),
            "/project",
        )
        .unwrap();
        let raw = decode(consumer, &serde_json::to_vec(&raw).unwrap(), "/project").unwrap();
        assert_eq!(normalized.operation, raw.operation);
        assert_eq!(normalized.cwd, raw.cwd);
    }
    for name in [
        "apply_patch",
        "write_stdin",
        "functions.apply_patch",
        "functions.write_stdin",
    ] {
        let event = decode(
            Consumer::Codex,
            &serde_json::to_vec(&json!({"name":name,"arguments":{}})).unwrap(),
            "/project",
        )
        .unwrap();
        assert!(matches!(
            event.operation,
            agent_guard_rust::adapters::Operation::Outside(_)
        ));
    }
}
