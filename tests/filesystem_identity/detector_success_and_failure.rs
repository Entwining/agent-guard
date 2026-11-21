#[path = "../support/cases/detector_success_and_failure.rs"]
pub mod cases;
use cases::*;

#[test]
fn lexical_protection_precedes_probe() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    // A single App Data file is allowed, but its lexical identity still
    // settles the decision before any probe.
    for (path, expected) in [
        (format!("{}/data.txt", fixture.container), "N"),
        (format!("{}/.ssh/id_rsa", fixture.home), "D"),
        (format!("{}/.env", fixture.project), "D"),
        (
            format!("/System/Volumes/Data{}/data.txt", fixture.container),
            "N",
        ),
    ] {
        let body = serde_json::to_vec(&json!({"tool_name":"Read","tool_input":{"file_path":path}}))
            .unwrap();
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        let result = evaluate(Event {
            bytes: &body,
            context: &ctx,
            probe: &mut probe,
        });
        assert_eq!(support::class(&result), expected, "{path}");
        assert!(
            probe.calls.is_empty() && probe.stat_calls.is_empty(),
            "{path}"
        );
    }
}

#[test]
fn identity_depth_bound_is_not_syntax_or_success() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
    probe
        .links
        .insert(format!("{}/loop-a", fixture.project), "loop-b".into());
    probe
        .links
        .insert(format!("{}/loop-b", fixture.project), "loop-a".into());
    let result = check(&fixture, &ctx, &mut probe, "cat loop-a");
    assert!(matches!(
        result,
        Ok(Evaluation {
            outcome: Outcome::CoverageInsufficient {
                cause: CoverageGap::IdentityBound,
            },
            ..
        })
    ));
    let wire = render(ctx.consumer, &result);
    assert_eq!((wire.exit, wire.stderr.as_str()), (0, ""));
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
        support::class(&check(&fixture, &ctx, &mut public, "cat link-0")),
        "N"
    );
}

#[test]
fn probe_fault_is_not_permission() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
    probe.fault = Some(fixture.project.clone());
    let result = check(&fixture, &ctx, &mut probe, "cat input.txt");
    assert_eq!(result.unwrap_err().kind, CheckErrorKind::ProbeFault);
    probe.fault = None;
    let restored = check(&fixture, &ctx, &mut probe, "cat input.txt");
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
