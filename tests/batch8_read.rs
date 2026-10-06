#[path = "support/batch6.rs"]
mod batch8;

#[test]
fn read_writes_every_destination() {
    batch8::partition("replace", include_str!("fixtures/rust-batch8-read.json"));
}

#[test]
fn read_options_write_the_named_variables() {
    batch8::partition("options", include_str!("fixtures/rust-batch8-read.json"));
}

#[test]
fn rejected_read_options_and_armed_fields_remain_protected() {
    batch8::partition("kept", include_str!("fixtures/rust-batch8-read.json"));
}

#[test]
fn unmodeled_read_forms_preserve_prior_refusals() {
    batch8::partition("fallback", include_str!("fixtures/rust-batch8-read.json"));
}

#[test]
fn missing_read_option_values_keep_the_public_failure_contract() {
    batch8::partition("control", include_str!("fixtures/rust-batch8-read.json"));
}

#[test]
fn read_field_split_preserves_the_last_remainder() {
    let packet: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/rust-batch8-read.json")).unwrap();
    for row in packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["partition"] == "record")
    {
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
            let values: Vec<_> = command.argv[command.program.unwrap() + 2..]
                .iter()
                .map(|word| word.text.as_str())
                .collect();
            let expected: Vec<_> = row["values"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect();
            assert_eq!(values, expected, "{consumer}: {}", row["id"]);
            assert!(
                command.argv[command.program.unwrap() + 2..]
                    .iter()
                    .all(|word| !word.expands),
                "{consumer}: known input stays known"
            );
        }
    }
}

#[test]
fn read_count_keeps_both_shell_values() {
    let packet: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/rust-batch8-read.json")).unwrap();
    let row = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "count-record")
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
        let values: Vec<_> = observation
            .script
            .commands
            .iter()
            .filter(|command| command.program.is_some_and(|i| command.argv[i] == "printf"))
            .map(|command| command.argv.last().unwrap().text.as_str())
            .collect();
        for expected in row["values"].as_array().unwrap() {
            assert!(
                values.contains(&expected.as_str().unwrap()),
                "{consumer}: both shells must survive: {values:?}"
            );
        }
    }
}
