#[path = "../support/cases/shell.rs"]
pub mod cases;
use cases::*;

#[test]
fn consumer_wire_regressions() {
    check_group("wire");
}
