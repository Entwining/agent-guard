use agent_guard_rust::entry;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn receipt(name: &str) -> PathBuf {
    let path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}", std::process::id()));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    path
}

#[test]
fn supervisor_deadline_kills_and_reaps_the_recorded_child() {
    let receipt = receipt("supervisor");
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-guard-rust-fixture-worker"));
    command
        .arg("bounded-stall")
        .arg(&receipt)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let started = Instant::now();
    assert_eq!(entry::supervise(&mut command).unwrap(), 137);
    let elapsed = started.elapsed();
    let pid = fs::read_to_string(&receipt)
        .unwrap()
        .trim()
        .strip_prefix("ready:")
        .unwrap()
        .to_owned();
    let alive = Command::new("/bin/kill")
        .args(["-0", &pid])
        .output()
        .unwrap();
    assert!(
        !alive.status.success(),
        "child {pid} remains alive or unreaped"
    );
    assert!(
        elapsed >= entry::SUPERVISOR_TIMEOUT && elapsed < Duration::from_millis(3300),
        "elapsed={elapsed:?}"
    );
    eprintln!(
        "child_pid={pid} kill_probe={} elapsed={elapsed:?}",
        alive.status
    );
    fs::remove_file(receipt).unwrap();
}

#[test]
fn supervisor_maps_signal_death_like_go() {
    let receipt = receipt("signal");
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-guard-rust-fixture-worker"));
    command
        .arg("signal")
        .arg(&receipt)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    assert_eq!(entry::supervise(&mut command).unwrap(), 143);
    fs::remove_file(receipt).unwrap();
}
