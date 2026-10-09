use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
};

#[test]
fn parallel_entry_calls_remain_silent_on_public_permits() {
    let root = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let bin = root.path().join("bin");
    let home = root.path().join("home");
    fs::create_dir(&bin).unwrap();
    fs::create_dir(&home).unwrap();
    let entry = bin.join("agent-guard");
    fs::copy("bin/agent-guard", &entry).unwrap();
    fs::set_permissions(&entry, fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink(
        env!("CARGO_BIN_EXE_agent-guard-native"),
        bin.join("agent-guard-native"),
    )
    .unwrap();
    let failures = std::thread::scope(|scope| {
        let workers = (0..8)
            .map(|worker| {
                let entry = &entry;
                let home = &home;
                scope.spawn(move || {
                    let mut failures = Vec::new();
                    for call in 0..1000 {
                        let mut child = Command::new(entry)
                            .args(["--runtime", "claude"])
                            .current_dir(home)
                            .env_clear()
                            .env("HOME", home)
                            .env("PATH", "/usr/bin:/bin")
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
                        if output.status.code() != Some(0)
                            || !output.stdout.is_empty()
                            || !output.stderr.is_empty()
                        {
                            failures.push((worker, call, output));
                        }
                    }
                    failures
                })
            })
            .collect::<Vec<_>>();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    println!("parallel entry: 8000 calls, {} failures", failures.len());
    assert!(
        failures.is_empty(),
        "public permits must be silent: {:?}",
        failures.iter().take(4).collect::<Vec<_>>()
    );
}
