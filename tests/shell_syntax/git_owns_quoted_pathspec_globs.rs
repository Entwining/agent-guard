#[path = "../support/cases/git_owns_quoted_pathspec_globs.rs"]
pub mod cases;
use cases::*;

#[test]
fn brace_union_and_nested_lists() {
    regressions("brace");
}

#[test]
fn quoted_parentheses_stay_literal() {
    regressions("paren");
}

#[test]
fn redirect_variables_follow_direction() {
    regressions("redirect");
    let observation = shell::observe(
        "cat < $SECRET > $SECRET <<< $SECRET",
        Arm::Brush,
        "/h",
        "/p",
        false,
    )
    .unwrap();
    let redirects = &observation.script.commands[0].redirects;
    for redirect in &redirects[..2] {
        assert!(matches!(redirect.direction, Direction::In | Direction::Out));
        assert!(redirect.vars.is_empty());
        assert!(redirect.expands);
    }
    assert_eq!(redirects[2].vars, ["SECRET"]);
}
