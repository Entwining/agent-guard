use std::process::Command;

#[test]
fn version_reports_the_release_owner() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-guard-rust-slice"))
        .arg("--version")
        .output()
        .expect("offline binary should start");
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        format!(
            "agent-guard-rust-slice {}\n",
            include_str!("../VERSION").trim()
        )
        .as_bytes()
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn unsupported_invocations_fail_without_stdout() {
    for arguments in [vec![], vec!["--checker"], vec!["--version", "extra"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-guard-rust-slice"))
            .args(&arguments)
            .output()
            .expect("offline binary should start");
        assert!(!output.status.success(), "arguments: {arguments:?}");
        assert!(output.stdout.is_empty(), "arguments: {arguments:?}");
        assert!(!output.stderr.is_empty());
    }
}
