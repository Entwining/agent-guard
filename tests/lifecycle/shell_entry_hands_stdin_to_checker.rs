use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

struct Package {
    root: PathBuf,
}

impl Package {
    fn new(name: &str) -> Self {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("shell-entry-{name}-{}", std::process::id()));
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("home")).unwrap();
        fs::copy("bin/agent-guard", root.join("bin/agent-guard")).unwrap();
        Self { root }
    }

    fn script_runner(&self, script: &str) {
        let runner = self.root.join("bin/agent-guard-native");
        fs::write(&runner, script).unwrap();
        fs::set_permissions(runner, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new(self.root.join("bin/agent-guard"));
        command
            .current_dir(self.root.join("home"))
            .env("HOME", self.root.join("home"))
            .env("AG_TEST_RECEIPT", self.root.join("checker"))
            .env(
                "AG_TEST_WORKER",
                env!("CARGO_BIN_EXE_agent-guard-rust-fixture-worker"),
            );
        command
    }

    fn event(&self, consumer: &str, event: &[u8]) -> Output {
        let mut child = self
            .command()
            .args(["--runtime", consumer])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let written = child.stdin.take().unwrap().write_all(event);
        let output = child.wait_with_output().unwrap();
        written.unwrap();
        output
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn shell_entry_hands_stdin_to_checker_byte_exactly() {
    let package = Package::new("stdin");
    package.script_runner(
        "#!/bin/bash -p\nexec \"$AG_TEST_WORKER\" runner capture-stdin \"$AG_TEST_RECEIPT\" \"$@\"\n",
    );
    let small = br#"{"tool_name":"Bash","tool_input":{"command":"true"}}"#.to_vec();
    let mut large = serde_json::to_vec(&serde_json::json!({
        "tool_name": "Bash", "tool_input": {"command": "true"},
        "padding": "p".repeat(96 * 1024),
    }))
    .unwrap();
    large.extend_from_slice(b"\n\n");
    let mut invalid = small.clone();
    invalid.insert(1, 0);
    for (name, event, status) in [
        ("without trailing newline", small, 0),
        ("over 64 KiB with trailing newlines", large, 0),
        ("embedded NUL", invalid, 2),
    ] {
        let output = package.event("claude", &event);
        let received = fs::read(package.root.join("checker.stdin")).unwrap();
        assert!(
            received == event,
            "{name}: checker received {} bytes; expected {} byte-exact bytes",
            received.len(),
            event.len()
        );
        assert_eq!(output.status.code(), Some(status), "{name}");
        assert!(output.stdout.is_empty(), "{name}");
        if status == 0 {
            assert!(output.stderr.is_empty(), "{name}");
        } else {
            assert!(String::from_utf8_lossy(&output.stderr).contains("guard failed"));
        }
    }
}

#[test]
fn shell_entry_keeps_outcomes_hook_cwd_and_version() {
    let package = Package::new("native");
    std::os::unix::fs::symlink(
        env!("CARGO_BIN_EXE_agent-guard-native"),
        package.root.join("bin/agent-guard-native"),
    )
    .unwrap();
    for consumer in ["claude", "codex", "pi"] {
        for (event, status, reason) in [
            (br#"{"tool_name":"Bash","tool_input":{"command":"true"}}"#.as_slice(), 0, ""),
            (br#"{"tool_name":"Read","tool_input":{"file_path":"Library/Containers/synthetic/data"}}"#.as_slice(), 2, "protected macOS app-data"),
            (br#"{"tool_name":"Bash","tool_input":{"command":"printenv"}}"#.as_slice(), 2, "dumps environment"),
            (b"{", 2, "could not complete its check (guard failed)"),
        ] {
            for trailing_newline in [false, true] {
                let mut bytes = event.to_vec();
                if trailing_newline {
                    bytes.push(b'\n');
                }
                let output = package.event(consumer, &bytes);
                assert_eq!(output.status.code(), Some(status), "{consumer} {reason}");
                assert!(output.stdout.is_empty());
                if status == 0 {
                    assert!(output.stderr.is_empty());
                } else {
                    assert!(String::from_utf8_lossy(&output.stderr).contains(reason));
                }
            }
        }
    }
    let version = package.command().arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        version.stdout,
        format!("agent-guard {}\n", include_str!("../../VERSION").trim()).as_bytes()
    );
    assert!(version.stderr.is_empty());
}

#[test]
fn shell_entry_only_accepts_completed_runner_statuses() {
    let advice: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/rust-refusal-advice.json")).unwrap();
    let package = Package::new("statuses");
    package.script_runner(
        "#!/bin/bash -p\nprintf '%s\\n' 'fixture reason'\nexit \"$AG_TEST_STATUS\"\n",
    );
    for status in [0, 3, 1, 2, 71, 101, 137] {
        let output = package
            .command()
            .args(["--runtime", "claude"])
            .env("AG_TEST_STATUS", status.to_string())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(if status == 0 { 0 } else { 2 }));
        match status {
            0 => {
                assert_eq!(output.stdout, b"fixture reason\n");
                assert!(output.stderr.is_empty());
            }
            3 => {
                assert!(output.stdout.is_empty());
                assert_eq!(output.stderr, b"fixture reason\n");
            }
            _ => {
                assert!(output.stdout.is_empty());
                assert!(String::from_utf8_lossy(&output.stderr).contains("guard failed"));
                assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture reason"));
                for field in ["reason", "alternative", "owner_step"] {
                    assert!(
                        String::from_utf8_lossy(&output.stderr)
                            .contains(advice["shell_fault"][field].as_str().unwrap())
                    );
                }
            }
        }
    }
}
