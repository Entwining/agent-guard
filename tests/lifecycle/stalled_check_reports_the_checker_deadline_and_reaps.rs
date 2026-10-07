use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("lifecycle-{name}-{}", std::process::id()));
        fs::create_dir_all(root.join("home")).unwrap();
        Self { root }
    }
    fn fault(&self, mode: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_agent-guard-rust-fixture-worker"))
            .args(["runner", mode])
            .arg(self.root.join("pid"))
            .env("HOME", self.root.join("home"))
            .output()
            .unwrap()
    }
    fn reaped(&self) {
        let receipt = fs::read_to_string(self.root.join("pid")).unwrap();
        let pid = receipt.trim().strip_prefix("ready:").unwrap();
        assert!(
            !Command::new("/bin/kill")
                .args(["-0", pid])
                .output()
                .unwrap()
                .status
                .success(),
            "recorded child {pid} remains alive or unreaped"
        );
        eprintln!("child_pid={pid} reaped=true");
    }
    fn native(&self, args: &[&str], home: &str, event: &[u8]) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agent-guard-rust-slice"))
            .args(args)
            .current_dir(&self.root)
            .env("HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(event).unwrap();
        child.wait_with_output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn stalled_check_reports_the_checker_deadline_and_reaps() {
    let fixture = Fixture::new("check-stall");
    let started = Instant::now();
    let output = fixture.fault("check-stall");
    let elapsed = started.elapsed();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("could not complete this check"));
    assert!(output.stderr.is_empty());
    let child_elapsed = Duration::from_micros(
        fs::read_to_string(fixture.root.join("pid.elapsed"))
            .unwrap()
            .parse()
            .unwrap(),
    );
    assert!(
        child_elapsed >= agent_guard_rust::entry::CHECKER_TIMEOUT
            && child_elapsed < Duration::from_millis(2800),
        "child_elapsed={child_elapsed:?}"
    );
    fixture.reaped();
    eprintln!("checker_deadline child_elapsed={child_elapsed:?} invocation_elapsed={elapsed:?}");
}

#[test]
fn checker_panic_never_becomes_a_permission() {
    let fixture = Fixture::new("panic");
    let output = fixture.fault("check-panic");
    assert_eq!(output.status.code(), Some(101));
    assert!(String::from_utf8_lossy(&output.stdout).contains("fixture checker panic"));
    assert!(output.stderr.is_empty());
    fixture.reaped();
}

#[test]
fn large_stdout_and_stderr_reasons_are_merged_without_deadlock() {
    let fixture = Fixture::new("large-reason");
    let output = fixture.fault("large-reason");
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout.len(), 4 * 1024 * 1024);
    assert!(output.stdout[..2 * 1024 * 1024].iter().all(|b| *b == b'O'));
    assert!(output.stdout[2 * 1024 * 1024..].iter().all(|b| *b == b'E'));
    fixture.reaped();
}

#[test]
fn malformed_input_fails_as_an_operational_error() {
    let fixture = Fixture::new("malformed");
    let output = fixture.native(
        &["--runtime", "claude", "--cwd", "/project"],
        fixture.root.join("home").to_str().unwrap(),
        b"{",
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("could not complete this check"));
}

#[test]
fn unresolved_relative_home_fails_before_evaluation() {
    let fixture = Fixture::new("home");
    let output = fixture.native(
        &["--runtime", "claude", "--cwd", "/project"],
        "absent-home",
        br#"{"tool_name":"Bash","tool_input":{"command":"true"}}"#,
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(!output.stdout.is_empty());
}

#[test]
fn unknown_runtime_never_reaches_the_checker() {
    let fixture = Fixture::new("runtime");
    let output = fixture.native(
        &["--runtime", "unknown", "--cwd", "/project"],
        fixture.root.join("home").to_str().unwrap(),
        b"{}",
    );
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        output.stdout,
        b"usage: agent-guard --runtime claude|codex|pi < event.json\n"
    );
}

#[test]
fn shell_entry_rejects_even_an_existing_relative_home() {
    let fixture = Fixture::new("wrapper-home");
    fs::create_dir_all(fixture.root.join("bin")).unwrap();
    let wrapper = fixture.root.join("bin/agent-guard");
    fs::copy("bin/agent-guard", &wrapper).unwrap();
    std::os::unix::fs::symlink(
        env!("CARGO_BIN_EXE_agent-guard-rust-slice"),
        fixture.root.join("bin/agent-guard-native"),
    )
    .unwrap();
    let mut child = Command::new(wrapper)
        .args(["--runtime", "claude"])
        .current_dir(&fixture.root)
        .env("HOME", "home")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"tool_name":"Bash","tool_input":{"command":"true"}}"#)
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("HOME is not an absolute path"));
}
