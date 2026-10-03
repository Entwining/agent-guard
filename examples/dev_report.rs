#[path = "../tests/support/mod.rs"]
mod support;
use agent_guard_rust::shell::Arm;

fn main() {
    let arm = match std::env::args().nth(1).as_deref() {
        Some("brush") => Arm::Brush,
        Some("tree") => Arm::TreeSitter,
        Some("structured") => Arm::StructuredOnly,
        _ => panic!("expected brush|tree|structured"),
    };
    for row in support::rows()
        .iter()
        .filter(|r| r["consumer"] != "owned-writer")
    {
        println!("{}", support::run(row, arm));
    }
}
