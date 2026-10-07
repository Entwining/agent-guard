#[path = "../support/cases/shell.rs"]
pub mod cases;
use cases::*;

#[test]
fn identity_review_regressions() {
    check_group("identity");
}
