#[path = "../support/cases/shell.rs"]
pub mod cases;
use cases::*;

#[test]
fn program_role_regressions() {
    check_group("program");
}

#[test]
fn metadata_role_regressions() {
    check_group("metadata");
}
