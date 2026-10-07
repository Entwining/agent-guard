#[path = "../support/cases/nested_parameter_flags_refuse_without_checker_faults.rs"]
pub mod cases;
use cases::*;

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
fn original_parameter_regions_have_supported_or_refused_coverage() {
    use agent_guard_rust::shell;
    let supported = shell::observe(
        "echo ${v:-x}",
        Arm::Brush,
        "/synthetic/home",
        "/synthetic/home/project",
        true,
    )
    .unwrap();
    assert_eq!(supported.word_coverage.len(), 1);
    let word = &supported.word_coverage[0];
    assert!(!word.unsupported);
    assert_eq!(word.parameters.len(), 1);
    assert_eq!(
        (
            word.parameters[0].range.clone(),
            word.parameters[0].supported
        ),
        (0..7, true)
    );
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
fn brush_backtick_leniency_keeps_the_input_and_refusal() {
    use agent_guard_rust::shell::{self, lexer::Lexed};
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("../fixtures/rust-m1-4-known-limits.json")).unwrap();
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
