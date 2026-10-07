#[path = "../support/cases/git_owns_quoted_pathspec_globs.rs"]
pub mod cases;
use cases::*;

#[test]
fn git_owns_quoted_pathspec_globs() {
    regressions("git");
}
