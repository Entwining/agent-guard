#[path = "../support/cases/credential_globs_and_roots_follow_the_lexical_owner.rs"]
pub mod cases;
use cases::*;

#[test]
fn gh_token_display_is_denied_at_the_secret_owner() {
    partition("gh");
}
