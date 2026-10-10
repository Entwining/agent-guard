use super::*;

fn fixture_dead(root: &Path) {
    let ids = fs::read_to_string(root.join("fixture-pids")).unwrap();
    assert!(!ids.trim().is_empty());
    for raw in ids.split_whitespace() {
        let pid = Pid::from_raw(raw.parse().unwrap());
        let deadline = Instant::now() + Duration::from_secs(1);
        while kill(pid, None).is_ok() {
            if Instant::now() >= deadline {
                let _ = kill(pid, Signal::SIGKILL);
                panic!("fixture process {raw} survived");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn fixture_elapsed(root: &Path) -> Duration {
    let started: u128 = fs::read_to_string(root.join("fixture-started"))
        .unwrap()
        .parse()
        .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    Duration::from_nanos(u64::try_from(now - started).unwrap())
}

#[test]
fn deadline_kills_descendants_and_removes_temporary_home() {
    let root = root();
    let entry = package(root.path(), "hang");
    let output = verifier(root.path(), &entry).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("deadline exceeded"), "{output:?}");
    assert!(out.contains(&format!("1/{} completed", protocol_cases().len())));
    // The 4.5-second call deadline and 0.5-second wait end the 20-second fixture near 5 seconds.
    assert!(fixture_elapsed(root.path()) < Duration::from_secs(10));
    fixture_dead(root.path());
    assert_clean(root.path());
}

#[test]
fn exited_child_with_inherited_output_pipes_is_bounded_and_cleaned() {
    let root = root();
    let entry = package(root.path(), "orphan-pipe");
    let output = verifier(root.path(), &entry).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("WaitDelay expired before I/O complete")
    );
    assert!(fixture_elapsed(root.path()) < Duration::from_secs(2));
    fixture_dead(root.path());
    assert_clean(root.path());
}

#[test]
fn interrupts_preserve_signal_exit_and_clean_up_child_group() {
    for (signal, code) in [
        (Signal::SIGTERM, 143),
        (Signal::SIGINT, 130),
        (Signal::SIGHUP, 129),
    ] {
        let root = root();
        let entry = package(root.path(), "hang");
        let mut child = verifier(root.path(), &entry)
            .process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = Pid::from_raw(child.id() as i32);
        let deadline = Instant::now() + Duration::from_secs(8);
        while !root.path().join("fixture-pids").exists() {
            if Instant::now() >= deadline {
                let _ = killpg(pid, Signal::SIGKILL);
                child.wait().unwrap();
                panic!("fixture never became ready");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if signal == Signal::SIGINT {
            killpg(pid, signal).unwrap();
        } else {
            kill(pid, signal).unwrap();
        }
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = killpg(pid, Signal::SIGKILL);
                child.wait().unwrap();
                panic!("interrupted verifier did not finish");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(code), "{output:?}");
        fixture_dead(root.path());
        assert_clean(root.path());
        let text = String::from_utf8(output.stdout).unwrap();
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            text.contains("FAIL\tclaude\tproject-shell\t0\t-1\t; signal killed; expected exit 0\n"),
            "stdout:\n{text}stderr:\n{error}"
        );
        assert!(
            text.ends_with(&format!(
                "Summary: 0 pass, 1 fail; 1/{} completed\n",
                protocol_cases().len()
            )),
            "stdout:\n{text}stderr:\n{error}"
        );
        assert_eq!(error, "FAIL\tacceptance interrupted or deadline exceeded\n");
    }
}

#[test]
fn interruption_after_direct_child_exit_keeps_the_result_and_cleans_descendants() {
    let root = root();
    let entry = package(root.path(), "orphan-pipe");
    let mut child = verifier(root.path(), &entry)
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = Pid::from_raw(child.id() as i32);
    let deadline = Instant::now() + Duration::from_secs(8);
    let direct = loop {
        if let Ok(ids) = fs::read_to_string(root.path().join("fixture-pids")) {
            break Pid::from_raw(ids.split_whitespace().next().unwrap().parse().unwrap());
        }
        if Instant::now() >= deadline {
            let _ = killpg(pid, Signal::SIGKILL);
            child.wait().unwrap();
            panic!("fixture never became ready");
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    while kill(direct, None).is_ok() {
        assert!(Instant::now() < deadline, "direct child was not reaped");
        std::thread::sleep(Duration::from_millis(2));
    }
    kill(pid, Signal::SIGTERM).unwrap();
    let output = child.wait_with_output().unwrap();
    fixture_dead(root.path());
    assert_clean(root.path());
    assert_eq!(output.status.code(), Some(143), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        text.contains("PASS\tclaude\tproject-shell\t0\t0\t-\n"),
        "stdout:\n{text}stderr:\n{error}"
    );
    assert!(
        text.ends_with(&format!(
            "Summary: 1 pass, 0 fail; 1/{} completed\n",
            protocol_cases().len()
        )),
        "stdout:\n{text}stderr:\n{error}"
    );
    assert_eq!(error, "FAIL\tacceptance interrupted or deadline exceeded\n");
}

#[test]
fn later_interrupts_do_not_replace_the_first_signal_exit() {
    let root = root();
    let entry = package(root.path(), "signal-burst");
    let mut child = verifier(root.path(), &entry)
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = Pid::from_raw(child.id() as i32);
    let deadline = Instant::now() + Duration::from_secs(8);
    while !root.path().join("fixture-pids").exists() {
        if Instant::now() >= deadline {
            let _ = killpg(pid, Signal::SIGKILL);
            child.wait().unwrap();
            panic!("fixture never became ready");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    kill(pid, Signal::SIGTERM).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    kill(pid, Signal::SIGINT).unwrap();
    let output = child.wait_with_output().unwrap();
    fixture_dead(root.path());
    assert_clean(root.path());
    assert_eq!(output.status.code(), Some(143), "{output:?}");
}
