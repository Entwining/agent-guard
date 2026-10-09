#[path = "../support/cases/detector_success_and_failure.rs"]
pub mod cases;
use cases::*;

#[test]
fn lexical_protection_precedes_probe() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    for path in [
        format!("{}/data.txt", fixture.container),
        format!("{}/.ssh/id_rsa", fixture.home),
        format!("{}/.env", fixture.project),
        format!("/System/Volumes/Data{}/data.txt", fixture.container),
    ] {
        let body = serde_json::to_vec(&json!({"tool_name":"Read","tool_input":{"file_path":path}}))
            .unwrap();
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        let result = evaluate_with_arm(
            Event {
                bytes: &body,
                context: &ctx,
                probe: &mut probe,
            },
            Arm::Brush,
        );
        assert_eq!(support::class(&result), "D");
        assert!(probe.calls.is_empty() && probe.stat_calls.is_empty());
    }
}

#[test]
fn identity_depth_bound_is_not_syntax_or_success() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        probe
            .links
            .insert(format!("{}/loop-a", fixture.project), "loop-b".into());
        probe
            .links
            .insert(format!("{}/loop-b", fixture.project), "loop-a".into());
        let result = check(&fixture, &ctx, &mut probe, arm, "cat loop-a");
        let Ok(Evaluation {
            outcome:
                Outcome::CoverageInsufficient {
                    recovery: Some(recovery),
                    ..
                },
            ..
        }) = &result
        else {
            panic!("missing identity-bound recovery");
        };
        assert!(
            recovery.excluded_scope.iter().any(|scope| scope
                == "unresolved resource identity from bounded or cyclic alias traversal")
        );
        assert!(
            render(ctx.consumer, &result)
                .stderr
                .contains("explicit public path")
        );
        assert!(matches!(
            result,
            Ok(Evaluation {
                outcome: Outcome::CoverageInsufficient {
                    cause: CoverageGap::IdentityBound,
                    disposition: Disposition::RejectUnsupportedSyntax,
                    ..
                },
                ..
            })
        ));
        assert_eq!(
            probe
                .calls
                .iter()
                .filter(|p| p.ends_with("loop-a") || p.ends_with("loop-b"))
                .count(),
            9
        );
        let mut public = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        for index in 0..8 {
            public.links.insert(
                format!("{}/link-{index}", fixture.project),
                if index == 7 {
                    "input.txt".into()
                } else {
                    format!("link-{}", index + 1)
                },
            );
        }
        assert_eq!(
            support::class(&check(&fixture, &ctx, &mut public, arm, "cat link-0")),
            "N"
        );
    }
}

#[test]
fn probe_fault_is_not_permission() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
    probe.fault = Some(fixture.project.clone());
    let result = check(&fixture, &ctx, &mut probe, Arm::Brush, "cat input.txt");
    assert_eq!(result.unwrap_err().kind, CheckErrorKind::ProbeFault);
    probe.fault = None;
    let restored = check(&fixture, &ctx, &mut probe, Arm::Brush, "cat input.txt");
    assert_eq!(support::class(&restored), "N");
    let mut gate = support::Gate {
        operation_start_count: 0,
    };
    assert!(
        gate.run(
            ctx.consumer,
            &agent_guard_rust::adapters::render(ctx.consumer, &restored),
            || std::fs::read_to_string(format!("{}/input.txt", fixture.project)).unwrap()
        )
        .unwrap()
        .contains("needle")
    );
}
