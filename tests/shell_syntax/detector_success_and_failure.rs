#[path = "../support/cases/detector_success_and_failure.rs"]
pub mod cases;
use cases::*;

#[test]
fn detector_success_and_failure() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        let success =
            shell::observe("setopt SH_WORD_SPLIT; printf ok", arm, "/h", "/h/p", true).unwrap();
        assert!(success.parse_successes > 0);
        assert!(success.gaps.is_empty());
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        let result = check(
            &fixture,
            &ctx,
            &mut probe,
            arm,
            "setopt SH_WORD_SPLIT; printf ok",
        )
        .unwrap();
        assert!(matches!(
            result.outcome,
            Outcome::CoverageInsufficient {
                cause: CoverageGap::ExecutorDivergence,
                disposition: Disposition::RejectUnsupportedSyntax,
                ..
            }
        ));
        let wire = render(ctx.consumer, &Ok(result));
        assert_eq!(wire.exit, 2);
        assert!(wire.stdout.is_empty() && wire.stderr.contains("recheck"));
        let failure =
            shell::observe("setopt SH_WORD_SPLIT; cat >", arm, "/h", "/h/p", true).unwrap();
        assert!(failure.parse_failures > 0);
        assert_eq!(failure.gaps, [CoverageGap::UnsupportedShellSyntax]);
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        let result = check(
            &fixture,
            &ctx,
            &mut probe,
            arm,
            "setopt SH_WORD_SPLIT; cat >",
        )
        .unwrap();
        assert!(matches!(
            result.outcome,
            Outcome::CoverageInsufficient {
                cause: CoverageGap::UnsupportedShellSyntax,
                disposition: Disposition::RejectUnsupportedSyntax,
                ..
            }
        ));
        let wire = render(ctx.consumer, &Ok(result));
        assert_eq!(wire.exit, 2);
        assert!(
            wire.stdout.is_empty()
                && wire.stderr.contains("explicit paths")
                && !wire.stderr.contains("checker failed")
        );
        let inert = shell::observe("printf '%s' '${(f)v}'", arm, "/h", "/h/p", true).unwrap();
        assert!(inert.gaps.is_empty());
    }
}

#[test]
fn alternative_argv_union_preserves_roles() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        for source in [
            format!(
                "p='needle {}/data.txt'; rg $p '{}'",
                fixture.container, fixture.project
            ),
            format!("p='needle -e'; rg $p '{}/data.txt'", fixture.container),
        ] {
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            assert_eq!(
                support::class(&check(&fixture, &ctx, &mut probe, arm, &source)),
                "D"
            );
        }
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        assert_eq!(
            support::class(&check(
                &fixture,
                &ctx,
                &mut probe,
                arm,
                &format!(
                    "rg -F 'needle {}/data.txt' '{}'",
                    fixture.container, fixture.project
                )
            )),
            "N"
        );
    }
}

#[test]
fn glob_group_position_and_body() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        for group in ["+(a|b).txt", "*(.)", "!(x)", "+(e:).txt"] {
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            assert_eq!(
                support::class(&check(
                    &fixture,
                    &ctx,
                    &mut probe,
                    arm,
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
            arm,
            &format!("printf '%s' {}/*(+fixture_filter)", fixture.project),
        );
        assert_eq!(support::class(&result), "UR");
        let Outcome::CoverageInsufficient {
            recovery: Some(recovery),
            ..
        } = &result.as_ref().unwrap().outcome
        else {
            panic!("missing qualifier recovery")
        };
        assert!(
            recovery
                .excluded_scope
                .iter()
                .any(|scope| scope.contains("Zsh executable qualifier"))
        );
        assert!(format!("{:?}", recovery.next_step).contains("qualifier"));
    }
}

#[test]
fn inspection_budget_bounds_function_expansion() {
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        let source = (0..9)
            .map(|i| format!("f{i}() {{ f{n}; f{n}; }}; ", n = i + 1))
            .collect::<String>()
            + "f9() { true; }; f0";
        let obs = shell::observe(&source, arm, "/h", "/h/p", true).unwrap();
        assert!(obs.gaps.contains(&CoverageGap::InspectionBudget));
        assert!(obs.script.commands.len() <= 512);
        let fixture = support::Fixture::new();
        let ctx = context(&fixture);
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        let result = check(&fixture, &ctx, &mut probe, arm, &source);
        assert!(
            render(ctx.consumer, &result)
                .stderr
                .contains("explicit public path")
        );
        let declared = shell::observe(
            "f() { cat /h/Library/Containers/c/data; }; printf ok",
            arm,
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
}
