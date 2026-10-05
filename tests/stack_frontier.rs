mod support;
use agent_guard_rust::shell::Arm;
use std::{
    fs::File,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn nesting_frontier_completes_on_default_test_stack() {
    const CHILD: &str = "AG_M2_FRONTIER_TEST_CHILD";
    if std::env::var(CHILD).as_deref() == Ok("1") {
        for row in support::rows().iter().filter(|r| {
            r["id"]
                .as_str()
                .unwrap()
                .starts_with("S20-unfrozen-nesting-")
        }) {
            support::assert_tuple(row, &support::run(row, Arm::Brush));
        }
        return;
    }
    // A fatal stack overflow must fail this assertion without aborting the observer.
    let log = std::path::PathBuf::from(std::env::var("CARGO_TARGET_DIR").unwrap())
        .parent()
        .unwrap()
        .join(format!("stack-frontier-{}.log", std::process::id()));
    let output = File::create(&log).unwrap();
    let started = Instant::now();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "nesting_frontier_completes_on_default_test_stack",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .stdout(Stdio::from(output.try_clone().unwrap()))
        .stderr(Stdio::from(output))
        .spawn()
        .unwrap();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let killed = child.kill();
                let reaped = child.wait();
                panic!("frontier wait failed: {error}; kill={killed:?}; wait={reaped:?}");
            }
        }
        if started.elapsed() > Duration::from_secs(10) {
            let killed = child.kill();
            let reaped = child.wait();
            panic!(
                "nesting frontier exceeded its observer deadline; kill={killed:?}; wait={reaped:?}; log: {}",
                log.display()
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(
        status.success(),
        "nesting frontier child failed with {status}; log: {}: {}",
        log.display(),
        std::fs::read_to_string(&log).unwrap()
    );
    std::fs::remove_file(&log).unwrap();
    assert!(
        !log.try_exists().unwrap(),
        "successful frontier log retained: {}",
        log.display()
    );
    println!(
        "nesting frontier child reaped in {:?}; successful log removed",
        started.elapsed()
    );
}
