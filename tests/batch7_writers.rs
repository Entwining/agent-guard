#[path = "support/batch6.rs"]
mod batch7;

#[test]
fn read_names_replace_armed_bindings() {
    batch7::partition("read", include_str!("fixtures/rust-batch7-writers.json"));
}

#[test]
fn printf_destination_replaces_armed_binding() {
    batch7::partition("printf", include_str!("fixtures/rust-batch7-writers.json"));
}

#[test]
fn function_local_names_replace_and_restore_bindings() {
    batch7::partition("local", include_str!("fixtures/rust-batch7-writers.json"));
}

#[test]
fn writer_models_preserve_other_armed_consumptions() {
    batch7::partition("kept", include_str!("fixtures/rust-batch7-writers.json"));
}

#[test]
fn eof_name_write_produces_a_known_empty_value() {
    let packet: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/rust-batch7-writers.json")).unwrap();
    let row = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["partition"] == "record")
        .unwrap();
    for consumer in ["claude", "codex", "pi"] {
        let observation = agent_guard_rust::shell::observe(
            row["input"]["command"].as_str().unwrap(),
            agent_guard_rust::shell::Arm::Brush,
            "/synthetic/home",
            "/synthetic/home/project",
            consumer != "pi",
        )
        .unwrap();
        let command = observation
            .script
            .commands
            .iter()
            .find(|command| command.program.is_some_and(|i| command.argv[i] == "printf"))
            .unwrap();
        let value = &command.argv[command.program.unwrap() + 2];
        assert_eq!(
            value.text, "",
            "{consumer}: EOF must replace the prior value"
        );
        assert!(
            !value.expands,
            "{consumer}: EOF is known data, not runtime uncertainty"
        );
    }
}
