use crate::support;
use agent_guard_rust::shell::Arm;
use std::{
    fs::File,
    io::{Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn nesting_frontier_completes_on_default_test_stack() {
    const CHILD: &str = "AG_M2_FRONTIER_TEST_CHILD";
    let rows: Vec<_> = support::rows()
        .into_iter()
        .filter(|r| {
            r["id"]
                .as_str()
                .unwrap()
                .starts_with("S20-unfrozen-nesting-")
        })
        .collect();
    assert!(!rows.is_empty(), "missing nesting frontier partition");
    let ids: Vec<_> = rows
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_owned())
        .collect();
    if std::env::var(CHILD).as_deref() == Ok("1") {
        for row in &rows {
            support::assert_tuple(row, &support::run(row, Arm::Brush));
        }
        println!("frontier_receipt={}", serde_json::to_string(&ids).unwrap());
        return;
    }
    // A fatal stack overflow must fail this assertion without aborting the observer.
    let log = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("stack-frontier-{}.log", std::process::id()));
    let output = File::create(&log).unwrap();
    let started = Instant::now();
    let name = module_path!().split_once("::").map_or_else(
        || "nesting_frontier_completes_on_default_test_stack".to_owned(),
        |(_, module)| format!("{module}::nesting_frontier_completes_on_default_test_stack"),
    );
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &name, "--nocapture"])
        .env(CHILD, "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::from(output))
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.read_to_end(&mut bytes).map(|_| bytes);
        let _ = send.send(result);
    });
    let bytes = match receive.recv_timeout(Duration::from_secs(15)) {
        Ok(bytes) => bytes,
        Err(error) => {
            let killed = child.kill();
            let reaped = child.wait();
            reader.join().unwrap();
            panic!(
                "frontier completion cap exceeded: {error}; kill={killed:?}; wait={reaped:?}; log: {}",
                log.display()
            );
        }
    };
    let status = child.wait().unwrap();
    reader.join().unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&log)
        .unwrap()
        .write_all(&bytes.unwrap())
        .unwrap();
    assert!(
        status.success(),
        "nesting frontier child failed with {status}; log: {}: {}",
        log.display(),
        std::fs::read_to_string(&log).unwrap()
    );
    let receipt = std::fs::read_to_string(&log).unwrap();
    // Serial libtest can prepend its test-name banner on the child's receipt line.
    let observed: Vec<String> = serde_json::from_str(
        receipt
            .lines()
            .find_map(|line| line.split_once("frontier_receipt=").map(|(_, json)| json))
            .unwrap_or_else(|| panic!("child must report every completed frontier observation")),
    )
    .unwrap();
    assert_eq!(observed, ids);
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
