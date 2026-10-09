use super::*;

pub(super) fn worker_fault(fixture: &Fixture, kind: CheckErrorKind) -> Value {
    use std::{
        process::{Command, Stdio},
        time::Duration,
    };
    let binary = Fixture::worker_binary();
    let receipt = fixture.root.join("worker-receipt");
    let started = Instant::now();
    let mut child = Command::new(binary)
        .arg(if kind == CheckErrorKind::GuardFault {
            "fault"
        } else {
            "wait"
        })
        .arg(&receipt)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    while fs::read_to_string(&receipt).ok().as_deref() != Some(&format!("ready:{pid}\n"))
        && started.elapsed() < Duration::from_secs(1)
    {
        std::thread::sleep(Duration::from_millis(1));
    }
    let ready = fs::read_to_string(&receipt).ok().as_deref() == Some(&format!("ready:{pid}\n"));
    if !ready {
        child.kill().unwrap();
        child.wait().unwrap();
        panic!("worker failed to become ready within fixture deadline");
    }
    if kind != CheckErrorKind::GuardFault {
        if kind == CheckErrorKind::Deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        child.kill().unwrap();
    }
    let status = child.wait().unwrap();
    assert_eq!(child.try_wait().unwrap(), Some(status));
    assert!(!status.success());
    let receipt = fs::read_to_string(receipt).unwrap();
    assert_eq!(receipt, format!("ready:{pid}\n"));
    assert!(started.elapsed() < Duration::from_secs(3));
    json!({"pid":pid,"ready_receipt":true,"completion_observed_by_wait":true,"reaped":true,"successful":false,"wall_us":started.elapsed().as_micros()})
}
