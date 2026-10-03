#[path = "../tests/support/differential.rs"]
mod differential;
#[path = "../tests/support/mod.rs"]
mod support;
use agent_guard_rust::shell::Arm;

fn main() {
    let arm = match std::env::args().nth(1).as_deref() {
        Some("brush") => Arm::Brush,
        Some("tree") => Arm::TreeSitter,
        _ => panic!("expected brush|tree"),
    };
    for row in differential::report(arm) {
        println!("{row}");
    }
}
