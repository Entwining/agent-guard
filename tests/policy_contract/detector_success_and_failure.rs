#[path = "../support/cases/detector_success_and_failure.rs"]
pub mod cases;
use cases::*;

#[test]
fn listing_recursion_controls_broad_root() {
    let fixture = support::Fixture::new();
    for consumer in [Consumer::Claude, Consumer::Codex, Consumer::Pi] {
        let mut ctx = context(&fixture);
        ctx.consumer = consumer;
        ctx.zsh_executor = consumer != Consumer::Pi;
        for cwd in [&fixture.project, &fixture.home] {
            ctx.cwd = cwd.clone();
            for source in [
                "ls ~",
                "ls /",
                "ls ~/Library",
                "ls -ltr ~",
                "ls -ltr",
                "ls",
                "if true; then ls; fi",
                "ls > out.txt",
                "ls -- -R ~",
            ] {
                let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
                let result = check(&fixture, &ctx, &mut probe, source);
                assert_eq!(
                    support::class(&result),
                    "N",
                    "{consumer:?} {source} in {cwd}"
                );
                assert_eq!(result.unwrap().coverage, Coverage::SupportedPreflight);
            }
        }
        ctx.cwd = fixture.home.clone();
        for option in ["-R", "--recursive", "-laR", "-Rl"] {
            for root in ["~", "~/Library", "/", "", "> out.txt"] {
                let source = format!("ls {option} {root}");
                let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
                let result = check(&fixture, &ctx, &mut probe, &source);
                assert_eq!(support::class(&result), "D", "{consumer:?} {source}");
                let Ok(Evaluation {
                    outcome: Outcome::ProtectedDenial { reason, recovery },
                    coverage,
                    ..
                }) = result
                else {
                    panic!("expected broad-root denial")
                };
                assert_eq!(coverage, Coverage::SupportedPreflight);
                assert!(reason.effect.contains("broad recursive"));
                assert!(!recovery.automatic_application_supported);
                assert!(
                    recovery
                        .excluded_scope
                        .iter()
                        .any(|scope| scope.contains("outside"))
                );
                assert_eq!(
                    agent_guard_rust::adapters::recovery_value(&recovery)["next_step"]["kind"],
                    "owner_action"
                );
                let next = json!({"tool":if consumer==Consumer::Pi {"bash"} else {"Bash"},"input":{"command":format!("ls '{}'",fixture.project)},"cwd":fixture.project});
                let body = serde_json::to_vec(
                    &json!({"tool_name":next["tool"],"tool_input":next["input"],"cwd":next["cwd"]}),
                )
                .unwrap();
                let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
                let rechecked = evaluate(Event {
                    bytes: &body,
                    context: &ctx,
                    probe: &mut probe,
                });
                assert_eq!(support::class(&rechecked), "N");
            }
        }
        ctx.cwd = fixture.project.clone();
        // Key names print no key contents; a container listing reaches
        // every file in it.
        for (path, expected) in [
            (format!("{}/.ssh", fixture.home), "N"),
            (fixture.container.clone(), "D"),
        ] {
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = check(&fixture, &ctx, &mut probe, &format!("ls '{path}'"));
            assert_eq!(support::class(&result), expected, "{path}");
            assert!(
                probe
                    .calls
                    .iter()
                    .all(|call| (call != &path || path.ends_with("/.ssh"))
                        && !call.starts_with(&format!("{path}/"))),
                "protected operand reached a probe"
            );
        }
    }
}

#[test]
fn protected_cwd_is_an_independent_owner() {
    let fixture = support::Fixture::new();
    let mut ctx = context(&fixture);
    ctx.cwd = fixture.container.clone();
    let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
    let result = check(&fixture, &ctx, &mut probe, "printf ok");
    assert_eq!(support::class(&result), "D");
    let Outcome::ProtectedDenial { reason, .. } = &result.as_ref().unwrap().outcome else {
        panic!("missing protected cwd denial")
    };
    assert!(reason.effect.contains("protected cwd"));
    assert!(
        render(ctx.consumer, &result)
            .stderr
            .contains(reason.rule.message())
    );
    assert!(probe.calls.is_empty());
}

#[test]
fn broad_root_recovery_preserves_excluded_scope() {
    let rows = support::rows();
    let selected: Vec<_> = rows
        .iter()
        .filter(|r| {
            r["id"]
                .as_str()
                .unwrap()
                .starts_with("S04-shell-home-explicit")
        })
        .collect();
    assert!(
        !selected.is_empty(),
        "missing explicit HOME recovery partition"
    );
    for row in selected {
        let result = support::run(row);
        support::assert_tuple(row, &result);
        let scope = result["recovery"]["excluded_scope"].to_string();
        assert!(
            scope.contains("outside")
                && scope.contains("Library")
                && scope.contains(".ssh")
                && scope.contains("environment-file")
        );
    }
}

#[test]
fn limited_coverage_is_quiet_on_wire() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
    let result = check(&fixture, &ctx, &mut probe, "fixture-unmodeled input.txt");
    assert!(
        matches!(&result,Ok(Evaluation { coverage:Coverage::LimitedPreflight(gaps),.. }) if gaps.contains(&CoverageGap::UnknownProgram { program:"fixture-unmodeled".into() }))
    );
    for consumer in [Consumer::Claude, Consumer::Codex, Consumer::Pi] {
        let wire = render(consumer, &result);
        assert_eq!(wire.exit, 0);
        assert!(wire.stdout.is_empty() && wire.stderr.is_empty());
    }
}

#[test]
fn command_boundaries_preserve_protected_operands() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    for source in [
        "cat \\\n  .env",
        "cat <<'EOF' | cat .env\npublic\nEOF",
        "cat <<'EOF' && cat .env\npublic\nEOF",
        "printf '%s' 'file '#1; env",
        "echo `cat .env`",
        "printf x | xargs cat .env",
        "git commit -F .env",
        "rg -nf .env README.md",
        "export -p",
        "export -px",
        "printf \"%s\" file\\\n#1; env",
        "cat <<'EOF' \\\n.env\nx\nEOF",
    ] {
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        assert_eq!(
            support::class(&check(&fixture, &ctx, &mut probe, source)),
            "D",
            "{source}"
        );
    }
    for source in [
        "export FOO=bar",
        "printenv HOME",
        "printf fixture > .env",
        "rg --files .env",
        "rg -uu --no-hidden -u needle src",
        "node -e 'console.log(e.key)'",
    ] {
        let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
        assert!(
            ["N", "UC"].contains(&support::class(&check(&fixture, &ctx, &mut probe, source))),
            "{source}"
        );
    }
}

#[test]
fn agent_continuation_preserves_chosen_search_data() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
    let result = check(
        &fixture,
        &ctx,
        &mut probe,
        &format!("rg -g '*.txt' -e 'different phrase' '{}'", fixture.home),
    );
    let recovery = match result.unwrap().outcome {
        Outcome::ProtectedDenial { recovery, .. } => recovery,
        _ => panic!("broad root permitted"),
    };
    let next = agent_guard_rust::adapters::recovery_value(&recovery);
    assert_eq!(next["next_step"]["kind"], "owner_action");
    assert!(next.get("objective").is_none());
    let operation = json!({"tool_name":"Grep","tool_input":{"path":fixture.project,"pattern":"different phrase","glob":"*.txt"},"cwd":fixture.project});
    let bytes = serde_json::to_vec(&operation).unwrap();
    let decoded = agent_guard_rust::adapters::decode(ctx.consumer, &bytes, &ctx.cwd).unwrap();
    assert_eq!(decoded.input["pattern"], "different phrase");
    assert_eq!(decoded.input["glob"], "*.txt");
    let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
    let result = evaluate(Event {
        bytes: &bytes,
        context: &ctx,
        probe: &mut probe,
    });
    assert_eq!(support::class(&result), "N");
    let wire = agent_guard_rust::adapters::render(ctx.consumer, &result);
    assert_eq!(wire.exit, 0);
    assert!(wire.stdout.is_empty());
    assert!(wire.stderr.is_empty());
}
