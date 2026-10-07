#[path = "../support/cases/shell.rs"]
pub mod cases;
use cases::*;

#[test]
fn shell_review_regressions() {
    check_group("shell");
}
