#[path = "../support/cases/detector_success_and_failure.rs"]
pub mod cases;
use cases::*;

#[test]
fn detector_success_and_failure() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    let success = shell::observe("setopt SH_WORD_SPLIT; printf ok", "/h", "/h/p", true).unwrap();
    assert!(success.parse_successes > 0);
    assert!(success.gaps.is_empty());
    let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
    let result = check(
        &fixture,
        &ctx,
        &mut probe,
        "setopt SH_WORD_SPLIT; printf ok",
    )
    .unwrap();
    assert!(matches!(
        result.outcome,
        Outcome::CoverageInsufficient {
            cause: CoverageGap::ExecutorDivergence,
        }
    ));
    let wire = render(ctx.consumer, &Ok(result));
    assert_eq!(
        (wire.exit, wire.stdout.as_str(), wire.stderr.as_str()),
        (0, "", "")
    );
    let failure = shell::observe("setopt SH_WORD_SPLIT; cat >", "/h", "/h/p", true).unwrap();
    assert!(failure.parse_failures > 0);
    assert_eq!(failure.gaps, [CoverageGap::UnsupportedShellSyntax]);
    let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
    let result = check(&fixture, &ctx, &mut probe, "setopt SH_WORD_SPLIT; cat >").unwrap();
    assert!(matches!(
        result.outcome,
        Outcome::CoverageInsufficient {
            cause: CoverageGap::UnsupportedShellSyntax,
        }
    ));
    let wire = render(ctx.consumer, &Ok(result));
    assert_eq!(
        (wire.exit, wire.stdout.as_str(), wire.stderr.as_str()),
        (0, "", "")
    );
    let inert = shell::observe("printf '%s' '${(f)v}'", "/h", "/h/p", true).unwrap();
    assert!(inert.gaps.is_empty());
}

#[test]
fn alternative_argv_union_preserves_roles() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    for source in [
        format!(
            "p='needle {}/data.txt'; rg $p '{}'",
            fixture.container, fixture.project
        ),
        format!("p='needle -e'; rg $p '{}/data.txt'", fixture.container),
    ] {
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        assert_eq!(
            support::class(&check(&fixture, &ctx, &mut probe, &source)),
            "D"
        );
    }
    let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
    assert_eq!(
        support::class(&check(
            &fixture,
            &ctx,
            &mut probe,
            &format!(
                "rg -F 'needle {}/data.txt' '{}'",
                fixture.container, fixture.project
            )
        )),
        "N"
    );
}

#[test]
fn glob_group_position_and_body() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    for group in ["+(a|b).txt", "*(.)", "!(x)", "+(e:).txt"] {
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        assert_eq!(
            support::class(&check(
                &fixture,
                &ctx,
                &mut probe,
                &format!("ls {}/{group}", fixture.project)
            )),
            "N",
            "{group}"
        );
    }
    let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
    let result = check(
        &fixture,
        &ctx,
        &mut probe,
        &format!("printf '%s' {}/*(+fixture_filter)", fixture.project),
    );
    assert_eq!(support::class(&result), "UC");
    assert!(render(ctx.consumer, &result).stderr.is_empty());
}

#[test]
fn inspection_budget_bounds_function_expansion() {
    let source = (0..9)
        .map(|i| format!("f{i}() {{ f{n}; f{n}; }}; ", n = i + 1))
        .collect::<String>()
        + "f9() { true; }; f0";
    let obs = shell::observe(&source, "/h", "/h/p", true).unwrap();
    assert!(obs.gaps.contains(&CoverageGap::InspectionBudget));
    assert!(obs.script.commands.len() <= 512);
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
    let result = check(&fixture, &ctx, &mut probe, &source);
    let wire = render(ctx.consumer, &result);
    assert_eq!((wire.exit, wire.stderr.as_str()), (0, ""));
    let declared = shell::observe(
        "f() { cat /h/Library/Containers/c/data; }; printf ok",
        "/h",
        "/h/p",
        true,
    )
    .unwrap();
    assert!(
        declared
            .script
            .commands
            .iter()
            .any(|c| c.argv.first().is_some_and(|s| s == "cat"))
    );
}
