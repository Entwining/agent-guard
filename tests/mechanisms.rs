mod support;
use agent_guard_rust::{
    CheckErrorKind, Context, Coverage, CoverageGap, Disposition, Evaluation, Event, Outcome,
    adapters::{Consumer, render},
    evaluate_with_arm,
    shell::{self, Arm},
};
use serde_json::{Value, json};
use std::cell::Cell;

fn context(fixture: &support::Fixture) -> Context {
    fixture.context(
        &support::rows()
            .into_iter()
            .find(|row| row["id"] == "S01-read-public-claude")
            .unwrap(),
    )
}
fn check(
    fixture: &support::Fixture,
    ctx: &Context,
    probe: &mut support::RecordingProbe,
    arm: Arm,
    source: &str,
) -> Result<Evaluation, agent_guard_rust::CheckError> {
    let body = serde_json::to_vec(&json!({"tool_name":if ctx.consumer==Consumer::Pi {"bash"} else {"Bash"},"tool_input":{"command":source}})).unwrap();
    let result = evaluate_with_arm(
        Event {
            bytes: &body,
            context: ctx,
            probe,
        },
        arm,
    );
    assert!(
        probe
            .calls
            .iter()
            .all(|path| !path.contains("/Containers/"))
    );
    let _ = fixture;
    result
}

#[test]
fn detector_success_and_failure() {
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        let success =
            shell::observe("setopt SH_WORD_SPLIT; printf ok", arm, "/h", "/h/p", true).unwrap();
        assert!(success.parse_successes > 0);
        assert!(success.gaps.contains(&CoverageGap::ExecutorDivergence));
        let failure =
            shell::observe("setopt SH_WORD_SPLIT; cat >", arm, "/h", "/h/p", true).unwrap();
        assert!(failure.parse_failures > 0);
        assert!(failure.gaps.contains(&CoverageGap::ExecutorDivergence));
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
            let mut probe = support::RecordingProbe::literal(&fixture);
            assert_eq!(
                support::class(&check(&fixture, &ctx, &mut probe, arm, &source)),
                "D"
            );
        }
        let mut probe = support::RecordingProbe::literal(&fixture);
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
            let mut probe = support::RecordingProbe::literal(&fixture);
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
        let mut probe = support::RecordingProbe::literal(&fixture);
        assert_eq!(
            support::class(&check(
                &fixture,
                &ctx,
                &mut probe,
                arm,
                &format!("printf '%s' {}/*(+fixture_filter)", fixture.project)
            )),
            "UR"
        );
    }
}

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
        let mut probe = support::RecordingProbe::literal(&fixture);
        let result = evaluate_with_arm(
            Event {
                bytes: &body,
                context: &ctx,
                probe: &mut probe,
            },
            Arm::Brush,
        );
        assert_eq!(support::class(&result), "D");
        assert!(probe.calls.is_empty());
    }
}

#[test]
fn identity_depth_bound_is_not_syntax_or_success() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        let mut probe = support::RecordingProbe::literal(&fixture);
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
                .contains("unresolved resource identity")
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
            40
        );
        let mut public = support::RecordingProbe::literal(&fixture);
        for index in 0..39 {
            public.links.insert(
                format!("{}/link-{index}", fixture.project),
                if index == 38 {
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
    let mut probe = support::RecordingProbe::literal(&fixture);
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

#[test]
fn interpreter_backslash_boundary() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        let mut probe = support::RecordingProbe::literal(&fixture);
        let escaped = check(
            &fixture,
            &ctx,
            &mut probe,
            arm,
            "python3 -c 'marker = \"路径\\.env\"'",
        );
        assert_eq!(support::class(&escaped), "UC");
        assert!(render(ctx.consumer, &escaped).stderr.is_empty());
        let plain = check(
            &fixture,
            &ctx,
            &mut probe,
            arm,
            "python3 -c 'marker = \"路径 .env\"'",
        );
        assert_eq!(support::class(&plain), "D");
    }
}

#[test]
fn name_only_listing_and_content_consumer() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        for listing in ["rg --files --hidden", "ls -a"] {
            for (tail, expected) in [
                ("", "N"),
                (" | xargs cat", "D"),
                ("; xargs cat input.txt", "N"),
            ] {
                let mut probe = support::RecordingProbe::literal(&fixture);
                let result = check(
                    &fixture,
                    &ctx,
                    &mut probe,
                    arm,
                    &format!("{listing} '{}'{}", fixture.project, tail),
                );
                assert_eq!(support::class(&result), expected, "{listing}{tail}");
            }
        }
    }
}

#[test]
fn listing_recursion_controls_broad_root() {
    let fixture = support::Fixture::new();
    for consumer in [Consumer::Claude, Consumer::Codex, Consumer::Pi] {
        let mut ctx = context(&fixture);
        ctx.consumer = consumer;
        ctx.zsh_executor = consumer != Consumer::Pi;
        for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
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
                    let mut probe = support::RecordingProbe::literal(&fixture);
                    let result = check(&fixture, &ctx, &mut probe, arm, source);
                    assert_eq!(
                        support::class(&result),
                        "N",
                        "{consumer:?} {arm:?} {source} in {cwd}"
                    );
                    assert_eq!(result.unwrap().coverage, Coverage::SupportedPreflight);
                }
            }
            ctx.cwd = fixture.home.clone();
            for option in ["-R", "--recursive", "-laR", "-Rl"] {
                for root in ["~", "~/Library", "/", "", "> out.txt"] {
                    let source = format!("ls {option} {root}");
                    let mut probe = support::RecordingProbe::literal(&fixture);
                    let result = check(&fixture, &ctx, &mut probe, arm, &source);
                    assert_eq!(
                        support::class(&result),
                        "D",
                        "{consumer:?} {arm:?} {source}"
                    );
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
                    let body = serde_json::to_vec(&json!({"tool_name":next["tool"],"tool_input":next["input"],"cwd":next["cwd"]})).unwrap();
                    let mut probe = support::RecordingProbe::literal(&fixture);
                    let rechecked = evaluate_with_arm(
                        Event {
                            bytes: &body,
                            context: &ctx,
                            probe: &mut probe,
                        },
                        arm,
                    );
                    assert_eq!(support::class(&rechecked), "N");
                }
            }
            ctx.cwd = fixture.project.clone();
            for path in [format!("{}/.ssh", fixture.home), fixture.container.clone()] {
                let mut probe = support::RecordingProbe::literal(&fixture);
                let result = check(&fixture, &ctx, &mut probe, arm, &format!("ls '{path}'"));
                assert_eq!(support::class(&result), "D", "{path}");
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
}

#[test]
fn protected_cwd_is_an_independent_owner() {
    let fixture = support::Fixture::new();
    let mut ctx = context(&fixture);
    ctx.cwd = fixture.container.clone();
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        let mut probe = support::RecordingProbe::literal(&fixture);
        let result = check(&fixture, &ctx, &mut probe, arm, "printf ok");
        assert_eq!(support::class(&result), "D");
        let reason = render(ctx.consumer, &result).stderr;
        assert!(reason.contains("protected cwd"));
        assert!(probe.calls.is_empty());
    }
}

#[test]
fn broad_root_recovery_preserves_excluded_scope() {
    for row in support::rows().iter().filter(|r| {
        r["id"]
            .as_str()
            .unwrap()
            .starts_with("S04-shell-home-explicit")
    }) {
        let result = support::run(row, Arm::Brush);
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
    let mut probe = support::RecordingProbe::literal(&fixture);
    let result = check(
        &fixture,
        &ctx,
        &mut probe,
        Arm::Brush,
        "fixture-unmodeled input.txt",
    );
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
fn gate_a_zero_starts_and_bypass_negative() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    let mut probe = support::RecordingProbe::literal(&fixture);
    let result = check(
        &fixture,
        &ctx,
        &mut probe,
        Arm::Brush,
        &format!("cat '{}/data.txt'", fixture.container),
    );
    assert_eq!(support::class(&result), "D");
    let read_count = Cell::new(0);
    let operation = || {
        read_count.set(read_count.get() + 1);
        std::fs::read_to_string(format!("{}/data.txt", fixture.container)).unwrap()
    };
    let mut gate = support::Gate {
        operation_start_count: 0,
    };
    let wire = agent_guard_rust::adapters::render(ctx.consumer, &result);
    assert!(gate.run(ctx.consumer, &wire, operation).is_none());
    assert_eq!(gate.operation_start_count, 0);
    assert_eq!(read_count.get(), 0);
    let mut bypass = support::Gate {
        operation_start_count: 0,
    };
    let mut broken_wire = wire;
    broken_wire.exit = 0;
    assert!(bypass.run(ctx.consumer, &broken_wire, operation).is_some());
    assert_eq!(read_count.get(), 1);
    let rejected = std::panic::catch_unwind(|| assert_eq!(bypass.operation_start_count, 0));
    assert!(rejected.is_err());
}

#[test]
fn adapters_raw_and_normalized_identity() {
    use agent_guard_rust::adapters::decode;
    let cases: [(Consumer, Value, Value); 3] = [
        (
            Consumer::Claude,
            json!({"tool_name":"Read","tool_input":{"file_path":"/project/input.txt"}}),
            json!({"tool_name":"Read","tool_input":{"file_path":"/project/input.txt"}}),
        ),
        (
            Consumer::Codex,
            json!({"tool_name":"Bash","tool_input":{"command":"cat /project/input.txt"}}),
            json!({"tool_name":"exec_command","arguments":"{\"cmd\":\"cat /project/input.txt\"}"}),
        ),
        (
            Consumer::Pi,
            json!({"tool_name":"read","tool_input":{"path":"/project/input.txt"}}),
            json!({"type":"tool_call","toolName":"read","input":{"path":"/project/input.txt"}}),
        ),
    ];
    for (consumer, normalized, raw) in cases {
        let normalized = decode(
            consumer,
            &serde_json::to_vec(&normalized).unwrap(),
            "/project",
        )
        .unwrap();
        let raw = decode(consumer, &serde_json::to_vec(&raw).unwrap(), "/project").unwrap();
        assert_eq!(normalized.operation, raw.operation);
        assert_eq!(normalized.cwd, raw.cwd);
    }
    for name in [
        "apply_patch",
        "write_stdin",
        "functions.apply_patch",
        "functions.write_stdin",
    ] {
        let event = decode(
            Consumer::Codex,
            &serde_json::to_vec(&json!({"name":name,"arguments":{}})).unwrap(),
            "/project",
        )
        .unwrap();
        assert!(matches!(
            event.operation,
            agent_guard_rust::adapters::Operation::Outside(_)
        ));
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
        let mut probe = support::RecordingProbe::literal(&fixture);
        let result = check(&fixture, &ctx, &mut probe, arm, &source);
        assert!(
            render(ctx.consumer, &result)
                .stderr
                .contains("over-budget function expansion")
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

#[test]
fn command_boundaries_preserve_protected_operands() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
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
            let mut probe = support::RecordingProbe::literal(&fixture);
            assert_eq!(
                support::class(&check(&fixture, &ctx, &mut probe, arm, source)),
                "D",
                "{arm:?} {source}"
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
            let mut probe = support::RecordingProbe::literal(&fixture);
            assert!(
                ["N", "UC"].contains(&support::class(&check(
                    &fixture, &ctx, &mut probe, arm, source
                ))),
                "{source}"
            );
        }
    }
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        let mut probe = support::RecordingProbe::literal(&fixture);
        assert_eq!(
            support::class(&check(
                &fixture,
                &ctx,
                &mut probe,
                arm,
                "repeat 2 do cat .env; done"
            )),
            "UR"
        );
    }
}

#[test]
fn agent_continuation_preserves_chosen_search_data() {
    let fixture = support::Fixture::new();
    let ctx = context(&fixture);
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        let mut probe = support::RecordingProbe::literal(&fixture);
        let result = check(
            &fixture,
            &ctx,
            &mut probe,
            arm,
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
        let mut probe = support::RecordingProbe::literal(&fixture);
        assert_eq!(
            support::class(&evaluate_with_arm(
                Event {
                    bytes: &bytes,
                    context: &ctx,
                    probe: &mut probe
                },
                arm
            )),
            "N"
        );
    }
}
