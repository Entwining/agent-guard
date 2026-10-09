#[path = "../support/cases/credential_globs_and_roots_follow_the_lexical_owner.rs"]
pub mod cases;
use cases::*;

#[test]
fn credential_globs_and_roots_follow_the_lexical_owner() {
    partition("credential");
}

#[test]
fn data_globs_touch_appdata() {
    partition("appdata");
}
