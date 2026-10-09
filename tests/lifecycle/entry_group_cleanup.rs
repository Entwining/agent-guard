use std::{
    fs,
    io::{BufRead, BufReader, Read},
    os::{
        fd::OwnedFd,
        unix::{fs::PermissionsExt, net::UnixStream, process::CommandExt},
    },
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

struct Package {
    root: tempfile::TempDir,
}

impl Package {
    /// The shell entry runs the fixture worker as its native runner.
    fn new() -> Self {
        let root = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        fs::create_dir(root.path().join("bin")).unwrap();
        fs::create_dir(root.path().join("home")).unwrap();
        let entry = root.path().join("bin/agent-guard");
        fs::copy("bin/agent-guard", &entry).unwrap();
        fs::set_permissions(&entry, fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(
            env!("CARGO_BIN_EXE_agent-guard-rust-fixture-worker"),
            root.path().join("bin/agent-guard-native"),
        )
        .unwrap();
        Self { root }
    }

    fn entry(&self, mode: &str) -> Command {
        let home = self.root.path().join("home");
        let mut command = Command::new(self.root.path().join("bin/agent-guard"));
        command
            .args(["entry-group", mode])
            .arg(self.root.path())
            .current_dir(&home)
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    /// Children write their own identities to the entry's stdin connection.
    fn observed(&self, mode: &str, wait: Duration, group: bool) -> (Child, Receipts) {
        let (observer, child_receipt) = UnixStream::pair().unwrap();
        observer.set_read_timeout(Some(wait)).unwrap();
        let mut command = self.entry(mode);
        command.stdin(Stdio::from(OwnedFd::from(child_receipt)));
        if group {
            command.process_group(0);
        }
        let spawned = Instant::now();
        let child = command.spawn().unwrap();
        let mut receipts = Receipts {
            connection: BufReader::new(observer),
            records: Vec::new(),
            spawned,
            first: None,
        };
        for _ in 0..2 {
            let mut line = String::new();
            receipts.connection.read_line(&mut line).unwrap();
            receipts.first.get_or_insert_with(Instant::now);
            let record: serde_json::Value = serde_json::from_str(&line).unwrap();
            println!("child-issued receipt: {record}");
            receipts.records.push(record);
        }
        (child, receipts)
    }
}

struct Receipts {
    connection: BufReader<UnixStream>,
    records: Vec<serde_json::Value>,
    /// The runner deadline cannot start before the spawn, so EOF earlier than the
    /// deadline after it means the deadline was shortened.
    spawned: Instant,
    /// The first receipt follows the runner's group and deadline setup. A late
    /// read only shortens time measured from it, while the fresh package's exec
    /// cost before it stays outside the runner deadline.
    first: Option<Instant>,
}

impl Receipts {
    fn field(&self, role: &str, field: &str) -> i32 {
        let record = self.records.iter().find(|r| r["role"] == role).unwrap();
        record[field].as_i64().unwrap() as i32
    }

    /// Every recorded child shares the leader's group, and the connections they
    /// held reach EOF only once each of them has exited. Returns the time to EOF
    /// from the spawn and from the first receipt.
    fn closed_in_group(&mut self, leader: i32) -> (Duration, Duration) {
        for record in &self.records {
            assert_eq!(
                record["pgid"].as_i64(),
                Some(i64::from(leader)),
                "child group differs: {record}"
            );
        }
        let mut extra = Vec::new();
        assert_eq!(
            self.connection.read_to_end(&mut extra).unwrap(),
            0,
            "child connection remains open"
        );
        let closed = (self.spawned.elapsed(), self.first.unwrap().elapsed());
        println!("spawn and first receipt to EOF: {closed:?}");
        // macOS reports EPERM for a group of unreaped zombies; reaping ends it.
        let deadline = Instant::now() + Duration::from_secs(1);
        while nix::sys::signal::killpg(nix::unistd::Pid::from_raw(leader), None)
            != Err(nix::errno::Errno::ESRCH)
        {
            assert!(Instant::now() < deadline, "runner group survives");
            std::thread::sleep(Duration::from_millis(5));
        }
        closed
    }
}

fn blocked(output: &Output) {
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("could not complete its check (guard failed), so this call is blocked")
    );
}

#[test]
fn entry_completion_closes_recorded_descendant_connections() {
    let package = Package::new();
    let (child, mut receipts) = package.observed("complete", Duration::from_secs(4), false);
    let runner = receipts.field("checker", "ppid");
    assert_eq!(
        receipts.field("checker", "pgid"),
        runner,
        "runner must lead its group before starting the checker"
    );
    fs::write(package.root.path().join("observed"), []).unwrap();
    let released = Instant::now();
    let output = child.wait_with_output().unwrap();
    // The descendant shares the entry's output pipe and sleeps eight seconds.
    assert!(
        released.elapsed() < Duration::from_secs(2),
        "entry waited for its leftover descendant: {:?}",
        released.elapsed()
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"fixture complete\n");
    assert!(output.stderr.is_empty());
    receipts.closed_in_group(runner);
}

#[test]
fn entry_deadline_closes_recorded_descendant_connections() {
    let package = Package::new();
    let (child, mut receipts) = package.observed("hang", Duration::from_secs(4), false);
    let output = child.wait_with_output().unwrap();
    blocked(&output);
    let runner = receipts.field("runner", "pid");
    assert_eq!(receipts.field("runner", "pgid"), runner);
    let (from_spawn, from_receipt) = receipts.closed_in_group(runner);
    assert!(
        from_spawn >= Duration::from_secs(3),
        "from_spawn={from_spawn:?}"
    );
    assert!(
        from_receipt < Duration::from_millis(3500),
        "from_receipt={from_receipt:?}"
    );
}

#[test]
fn entry_cancellation_still_closes_the_runner_group_by_the_deadline() {
    for signal in [
        nix::sys::signal::Signal::SIGTERM,
        nix::sys::signal::Signal::SIGINT,
        nix::sys::signal::Signal::SIGHUP,
    ] {
        let package = Package::new();
        let (child, mut receipts) = package.observed("hang", Duration::from_secs(4), true);
        let entry_group = nix::unistd::Pid::from_raw(child.id() as i32);
        nix::sys::signal::killpg(entry_group, signal).unwrap();
        let output = child.wait_with_output().unwrap();
        assert_ne!(
            output.status.code(),
            Some(0),
            "{signal}: cancellation permitted"
        );
        let runner = receipts.field("runner", "pid");
        assert_ne!(
            runner,
            entry_group.as_raw(),
            "{signal}: runner kept the entry group"
        );
        let (_, from_receipt) = receipts.closed_in_group(runner);
        assert!(
            from_receipt < Duration::from_millis(3500),
            "{signal}: from_receipt={from_receipt:?}"
        );
    }
}

#[test]
fn runner_group_failure_never_becomes_a_permission() {
    let package = Package::new();
    let output = package
        .entry("session-leader")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    blocked(&output);
}
