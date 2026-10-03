#[path = "../tests/support/differential.rs"]
mod differential;
#[path = "../tests/support/mod.rs"]
mod support;
use agent_guard_rust::shell::Arm;

fn main() {
    let arm = match std::env::args().nth(1).as_deref() {
        Some("brush") => Arm::Brush,
        _ => panic!("expected brush"),
    };
    assert_eq!(arm, Arm::Brush);
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        for row in differential::report(arm) {
            println!("{row}");
        }
    }
}
