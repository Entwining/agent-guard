#[path = "../tests/support/differential.rs"]
mod differential;
#[path = "support/fixture_paths.rs"]
mod fixture_paths;
#[path = "../tests/support/harness.rs"]
mod support;
fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("brush") => {}
        _ => panic!("expected brush"),
    };
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        for row in differential::report(arm) {
            println!("{row}");
        }
    }
}
