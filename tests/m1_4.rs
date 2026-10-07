mod support;

use agent_guard_rust::{
    Coverage, CoverageGap, Disposition, Event, Outcome, adapters, evaluate_with_arm, shell::Arm,
};
use serde_json::{Value, json};

fn check_rows(controls: bool) {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/rust-m1-4-fragments.json")).unwrap();
    let fixture = support::Fixture::new();
    let mut failures = Vec::new();
    for consumer in ["claude", "codex", "pi"] {
        let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
        for row in rows
            .iter()
            .filter(|row| (row["owner"] == "parameter_fragment_control") == controls)
        {
            let body = serde_json::to_vec(&json!({
                "tool_name":if consumer == "pi" {"bash"} else {"Bash"},
                "tool_input":{"command":row["source"]}
            }))
            .unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate_with_arm(
                Event {
                    bytes: &body,
                    context: &context,
                    probe: &mut probe,
                },
                Arm::Brush,
            );
            let wire = adapters::render(context.consumer, &result);
            let class = support::class(&result);
            if class != row["expected"] {
                failures.push(json!({"id":row["id"],"consumer":consumer,"expected":row["expected"],"class":class,"result":format!("{result:?}"),"exit":wire.exit,"stderr":wire.stderr}));
                continue;
            }
            assert!(wire.stdout.is_empty());
            assert_eq!(wire.exit, if class == "N" { 0 } else { 2 });
            if class == "UR" {
                let evaluation = result.as_ref().unwrap();
                let Coverage::LimitedPreflight(gaps) = &evaluation.coverage else {
                    panic!("missing D1 coverage: {row}");
                };
                assert!(gaps.contains(&if consumer == "pi" {
                    CoverageGap::UnsupportedDialectConstruct
                } else {
                    CoverageGap::ExecutorDivergence
                }));
                assert!(matches!(
                    evaluation.outcome,
                    Outcome::CoverageInsufficient {
                        disposition: Disposition::RejectUnsupportedSyntax,
                        ..
                    }
                ));
                assert!(wire.stderr.contains("unsupported"));
                assert!(!wire.stderr.contains("checker failed"));
            }
        }
    }
    println!("{}", json!({"failures":failures}));
    assert!(
        failures.is_empty(),
        "{} regression failures",
        failures.len()
    );
}

#[test]
fn nested_parameter_flags_refuse_without_checker_faults() {
    check_rows(false);
}

#[test]
fn parameter_fragment_controls_keep_literal_and_nested_code_distinct() {
    check_rows(true);
}

#[test]
fn unterminated_parameter_sources_never_grant_permission() {
    let fixture = support::Fixture::new();
    for consumer in ["claude", "codex", "pi"] {
        let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
        for source in [
            "echo ${v:-${w}",
            "echo ${v:-${(f)v}",
            "echo ${v:-${(f)v",
            "echo ${v/x/${(f)v",
            "echo ${v:0:${(f)v",
            "echo $(( ${v",
        ] {
            let body = serde_json::to_vec(&json!({
                "tool_name":if consumer == "pi" {"bash"} else {"Bash"},
                "tool_input":{"command":source}
            }))
            .unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate_with_arm(
                Event {
                    bytes: &body,
                    context: &context,
                    probe: &mut probe,
                },
                Arm::Brush,
            );
            assert_eq!(support::class(&result), "UR", "{consumer}: {source}");
            let Coverage::LimitedPreflight(gaps) = &result.as_ref().unwrap().coverage else {
                panic!("missing malformed-input coverage: {source}");
            };
            assert!(gaps.contains(&CoverageGap::UnsupportedShellSyntax));
            let wire = adapters::render(context.consumer, &result);
            assert_eq!(wire.exit, 2);
            assert!(wire.stdout.is_empty());
        }
    }
}

fn mechanism_rows(owner: &str) {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/rust-m1-4-regressions.json")).unwrap();
    let fixture = support::Fixture::new();
    let mut failures = Vec::new();
    for consumer in ["claude", "codex", "pi"] {
        let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
        for row in rows.iter().filter(|r| r["owner"] == owner) {
            let source = fixture.expand(row["source"].as_str().unwrap());
            let bytes = serde_json::to_vec(&json!({"tool_name":if consumer == "pi" {"bash"} else {"Bash"},"tool_input":{"command":source}})).unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate_with_arm(
                Event {
                    bytes: &bytes,
                    context: &context,
                    probe: &mut probe,
                },
                Arm::Brush,
            );
            let wire = adapters::render(context.consumer, &result);
            let expected = if matches!(
                row["id"].as_str(),
                Some("i4-arith-sub-plain" | "i5-arith-sub-param")
            ) {
                "D" // Ruling 16 moves the two recorded arithmetic gaps to P2.
            } else {
                row["expected"].as_str().unwrap()
            };
            if support::class(&result) != expected
                || wire.exit != if matches!(expected, "N" | "UC") { 0 } else { 2 }
            {
                failures.push(json!({"id":row["id"],"consumer":consumer,"expected":row["expected"],"actual":support::class(&result),"result":format!("{result:?}"),"wire":format!("{wire:?}")}));
            }
            assert!(wire.stdout.is_empty());
            match &result.as_ref().unwrap().outcome {
                agent_guard_rust::Outcome::NoObjection => assert!(wire.stderr.is_empty()),
                agent_guard_rust::Outcome::ProtectedDenial { reason, recovery } => {
                    let environmental = matches!(
                        row["id"].as_str().unwrap(),
                        "A102-herestring-eval"
                            | "A043-herestring-direct"
                            | "A056-subscript-assign"
                            | "g1-plain-unquoted"
                            | "coverage-sibling"
                            | "coverage-nested-effect"
                            | "coverage-eval-queue"
                            | "coverage-qualifier-queue"
                            | "qualifier-nested-quotes"
                            | "i4-arith-sub-plain"
                            | "i5-arith-sub-param"
                    );
                    assert!(
                        reason.effect.contains(if environmental {
                            "environment"
                        } else {
                            "private-key"
                        }),
                        "{row}: {reason:?}"
                    );
                    assert!(wire.stderr.contains(reason.rule.message()));
                    assert!(!recovery.excluded_scope.is_empty());
                    assert!(!recovery.automatic_application_supported);
                    assert!(!wire.stderr.contains("recovery:"));
                }
                agent_guard_rust::Outcome::CoverageInsufficient {
                    cause, recovery, ..
                } => {
                    if row["expected"] == "UC" {
                        assert_eq!(cause, &CoverageGap::UnresolvedTarget);
                        assert!(recovery.is_none());
                        assert!(wire.stderr.is_empty());
                    } else {
                        let syntax = matches!(
                            row["id"].as_str().unwrap(),
                            "s3-idx-atZ"
                                | "s6-arith-atZ"
                                | "s9-legacy-atZ"
                                | "c3-top-atZ"
                                | "l3-trim-quoted-nested"
                                | "l4-default-nested"
                                | "l5-top-quoted-nested"
                        );
                        assert_eq!(
                            cause,
                            &if syntax {
                                CoverageGap::UnsupportedShellSyntax
                            } else if consumer == "pi" {
                                CoverageGap::UnsupportedDialectConstruct
                            } else {
                                CoverageGap::ExecutorDivergence
                            },
                            "{row}"
                        );
                        let recovery = recovery.as_ref().unwrap();
                        assert!(!recovery.excluded_scope.is_empty());
                        assert!(!recovery.automatic_application_supported);
                        assert!(if syntax {
                            wire.stderr.contains("shell syntax")
                                && wire.stderr.contains("explicit paths")
                        } else {
                            wire.stderr.contains("unsupported") && wire.stderr.contains("recheck")
                        });
                        assert!(!wire.stderr.contains("checker failed"));
                    }
                }
                other => panic!("unexpected advice/outcome: {row}: {other:?}"),
            }
        }
    }
    println!("{}", json!({"owner":owner,"failures":failures}));
    assert!(
        failures.is_empty(),
        "{owner} observation failures: {}",
        failures.len()
    );
}
#[test]
fn word_start_comments_keep_later_effects() {
    mechanism_rows("comments");
}
#[test]
fn herestring_keeps_later_lines_active() {
    mechanism_rows("herestring");
}
#[test]
fn escaped_dollar_keeps_brace_reach() {
    mechanism_rows("brace");
}
#[test]
fn parameter_region_coverage_and_wire() {
    mechanism_rows("coverage");
}
#[test]
fn fragments_preserve_single_quoted_data() {
    mechanism_rows("literal_fragment");
}
#[test]
fn array_assignment_is_not_process_substitution() {
    mechanism_rows("array_assignment");
}
#[test]
fn assignment_subscript_forwards_code() {
    mechanism_rows("assignment");
    let observation = agent_guard_rust::shell::observe(
        "a[$(cat .env)]=public",
        Arm::Brush,
        "/synthetic/home",
        "/synthetic/home/project",
        true,
    )
    .unwrap();
    assert!(
        observation
            .script
            .commands
            .iter()
            .any(|c| c.nested && c.argv == ["cat", ".env"]),
        "assignment subscript nested effect is visible"
    );
    let data = agent_guard_rust::shell::observe(
        "a[1]='$(cat .env)'",
        Arm::Brush,
        "/synthetic/home",
        "/synthetic/home/project",
        true,
    )
    .unwrap();
    assert!(
        data.script
            .commands
            .iter()
            .all(|c| c.program.is_none() && !c.nested),
        "assignment data is not executed code"
    );
}

#[test]
fn lexical_owner_dimensions_have_independent_witnesses() {
    use agent_guard_rust::shell::lexer::{Lexed, Quote};
    for source in [
        r"printf file\ #1 ${~v}",
        r"printf file\)#1 ${~v}",
        "echo $(echo x)#${~v}",
        "echo $((1))#${~v}",
        "echo <(echo x)#${~v}",
        "echo @(a)#${~v}",
        "(( 1 ))#${~v}",
    ] {
        let lex = Lexed::scan(source).unwrap();
        assert!(
            !lex.context(source.find('#').unwrap()).comment,
            "in-word #: {source}"
        );
        assert!(
            lex.context(source.find("${").unwrap()).active(),
            "later D1 region: {source}"
        );
    }
    let source = "(echo x)#${~v}";
    assert!(
        Lexed::scan(source)
            .unwrap()
            .context(source.find('#').unwrap())
            .comment
    );
    let source = "echo $((1 # 2))";
    assert!(
        !Lexed::scan(source)
            .unwrap()
            .context(source.find('#').unwrap())
            .comment
    );
    for source in ["cat <<< 'a b'\necho ${~v}", "cat <<<- 'a'\necho ${~v}"] {
        let c = Lexed::scan(source)
            .unwrap()
            .context(source.find("${").unwrap());
        assert!(
            c.active() && c.heredoc.is_none(),
            "here-string body: {source}"
        );
    }
    let source = "echo ${v:-$(echo `echo $((1))`)}";
    let lex = Lexed::scan(source).unwrap();
    let c = lex.context(source.find('1').unwrap());
    assert_eq!(
        (
            c.command_depth,
            c.parameter_depth,
            c.backtick_depth,
            c.arithmetic_depth
        ),
        (1, 1, 1, 1)
    );
    assert_eq!(c.quote, Quote::Unquoted);
    let source = "echo $[1 # ${v@Z}]";
    let lex = Lexed::scan(source).unwrap();
    assert_eq!(lex.context(source.find('#').unwrap()).arithmetic_depth, 1);
    assert!(!lex.context(source.find('#').unwrap()).comment);
    for source in ["cat <<'TAG'\necho x\nTAG", "cat <<TAG\necho x\nTAG"] {
        let lex = Lexed::scan(source).unwrap();
        assert!(lex.context(source.find("TAG").unwrap()).heredoc_delimiter);
        assert!(lex.context(source.rfind("TAG").unwrap()).heredoc_delimiter);
    }
}

#[test]
fn manifest_input_count_matches_its_declared_provenance() {
    let manifest: Value =
        serde_json::from_str(include_str!("fixtures/rust-m1-4-input-manifest.json")).unwrap();
    assert_eq!(
        manifest["inputs"].as_array().unwrap().len(),
        manifest["counts"]["inputs"].as_u64().unwrap() as usize
    );
}

#[test]
fn original_parameter_regions_have_supported_or_refused_coverage() {
    use agent_guard_rust::shell;
    for (body, expected, cause) in [
        (
            "${v@Z}",
            vec![(0..6, false)],
            CoverageGap::UnsupportedShellSyntax,
        ),
        (
            "${v[${v@Z}]}",
            vec![(0..12, true), (4..10, false)],
            CoverageGap::UnsupportedShellSyntax,
        ),
        (
            "${v:-${v@Z}}",
            vec![(0..12, false), (5..11, false)],
            CoverageGap::UnsupportedShellSyntax,
        ),
        (
            "${v:-${(f)v}}",
            vec![(0..13, false), (5..12, false)],
            CoverageGap::ExecutorDivergence,
        ),
        (
            "$((${v@Z}))",
            vec![(3..9, false)],
            CoverageGap::UnsupportedShellSyntax,
        ),
        (
            "$[${v@Z}]",
            vec![(2..8, false)],
            CoverageGap::UnsupportedShellSyntax,
        ),
        (
            "\"${${v}}\"",
            vec![(1..8, false), (3..7, true)],
            CoverageGap::UnsupportedShellSyntax,
        ),
    ] {
        let source = format!("echo {body}");
        let observation = shell::observe(
            &source,
            Arm::Brush,
            "/synthetic/home",
            "/synthetic/home/project",
            true,
        )
        .unwrap();
        assert_eq!(observation.gaps, [cause], "word refusal cause: {source}");
        assert_eq!(observation.word_coverage.len(), 1, "{source}");
        let word = &observation.word_coverage[0];
        assert!(word.unsupported);
        assert_eq!(
            word.parameters
                .iter()
                .map(|r| (r.range.clone(), r.supported))
                .collect::<Vec<_>>(),
            expected,
            "per-region support and span: {word:?}"
        );
    }
    for source in [
        "echo ${v:-${w:-x}}",
        "echo '${~v}'",
        "echo \\${x}",
        "echo $(echo ${w:-x})",
    ] {
        let observation = shell::observe(
            source,
            Arm::Brush,
            "/synthetic/home",
            "/synthetic/home/project",
            true,
        )
        .unwrap();
        assert!(
            observation
                .word_coverage
                .iter()
                .all(|w| !w.unsupported && w.parameters.iter().all(|r| r.supported)),
            "supported/literal control: {source}"
        );
        assert!(
            observation.gaps.is_empty(),
            "supported/literal control: {source}"
        );
    }
    let source = "echo '${inactive}' ${v@Z}";
    let observation = shell::observe(
        source,
        Arm::Brush,
        "/synthetic/home",
        "/synthetic/home/project",
        true,
    )
    .unwrap();
    assert_eq!(observation.gaps, [CoverageGap::UnsupportedShellSyntax]);
    assert_eq!(observation.word_coverage.len(), 1);
    let active = &observation.word_coverage[0];
    assert_eq!(active.raw, "${v@Z}");
    assert_eq!(
        active
            .parameters
            .iter()
            .map(|r| (r.range.clone(), r.supported))
            .collect::<Vec<_>>(),
        [(0..6, false)]
    );
}

#[test]
fn probe_fault_stays_fault_with_independently_observed_denial() {
    use agent_guard_rust::{CheckErrorKind, shell};
    struct Fault;
    impl agent_guard_rust::filesystem::Probe for Fault {
        fn stat(
            &mut self,
            _: &std::path::Path,
        ) -> std::io::Result<Option<agent_guard_rust::filesystem::Metadata>> {
            Err(std::io::ErrorKind::PermissionDenied.into())
        }
        fn read_link(
            &mut self,
            _: &std::path::Path,
        ) -> std::io::Result<Option<std::path::PathBuf>> {
            Err(std::io::ErrorKind::PermissionDenied.into())
        }
    }
    let source = "cat .env; echo ${v:-${(f)v}}; cat public";
    let observation = shell::observe(
        source,
        Arm::Brush,
        "/synthetic/home",
        "/synthetic/home/project",
        true,
    )
    .unwrap();
    assert!(
        observation
            .script
            .commands
            .iter()
            .any(|c| c.argv.iter().any(|w| w.text == ".env")),
        "independent protected operand survives refusal"
    );
    let context = agent_guard_rust::Context {
        consumer: adapters::Consumer::Claude,
        home: "/synthetic/home".into(),
        cwd: "/synthetic/home/project".into(),
        user: None,
        zsh_executor: true,
        require_execution_owner: false,
        shell_observation_entries: std::cell::Cell::new(0),
    };
    let bytes =
        serde_json::to_vec(&json!({"tool_name":"Bash","tool_input":{"command":source}})).unwrap();
    let result = evaluate_with_arm(
        Event {
            bytes: &bytes,
            context: &context,
            probe: &mut Fault,
        },
        Arm::Brush,
    );
    assert_eq!(result.unwrap_err().kind, CheckErrorKind::ProbeFault);
}

#[test]
fn brush_backtick_leniency_keeps_the_input_and_refusal() {
    use agent_guard_rust::shell::{self, lexer::Lexed};
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/rust-m1-4-known-limits.json")).unwrap();
    for row in rows {
        let source = row["source"].as_str().unwrap();
        assert!(
            brush_parser::uncached_tokenize_str(source, &brush_parser::TokenizerOptions::default())
                .is_ok()
        );
        assert!(Lexed::scan(source).is_err());
        let observation = shell::observe(
            source,
            Arm::Brush,
            "/synthetic/home",
            "/synthetic/home/project",
            true,
        )
        .unwrap();
        assert!(
            observation
                .gaps
                .contains(&CoverageGap::UnsupportedShellSyntax)
        );
        assert!(observation.script.commands.is_empty());
    }
}
