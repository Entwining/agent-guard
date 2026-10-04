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
            if support::class(&result) != row["expected"]
                || wire.exit
                    != if matches!(row["expected"].as_str(), Some("N" | "UC")) {
                        0
                    } else {
                        2
                    }
            {
                failures.push(json!({"id":row["id"],"consumer":consumer,"expected":row["expected"],"actual":support::class(&result),"result":format!("{result:?}"),"wire":format!("{wire:?}")}));
            }
            assert!(wire.stdout.is_empty());
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
    for source in ["cat <<'TAG'\necho x\nTAG", "cat <<TAG\necho x\nTAG"] {
        let lex = Lexed::scan(source).unwrap();
        assert!(lex.context(source.find("TAG").unwrap()).heredoc_delimiter);
        assert!(lex.context(source.rfind("TAG").unwrap()).heredoc_delimiter);
    }
}

struct NoIoProbe;
impl agent_guard_rust::filesystem::Probe for NoIoProbe {
    fn stat(
        &mut self,
        _: &std::path::Path,
    ) -> std::io::Result<Option<agent_guard_rust::filesystem::Metadata>> {
        Ok(None)
    }
    fn read_link(&mut self, _: &std::path::Path) -> std::io::Result<Option<std::path::PathBuf>> {
        Ok(None)
    }
}

#[test]
fn manifest_observations() {
    let manifest: Value =
        serde_json::from_str(include_str!("fixtures/rust-m1-4-input-manifest.json")).unwrap();
    let rows = manifest["inputs"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        manifest["counts"]["inputs"].as_u64().unwrap() as usize
    );
    for consumer in ["claude", "codex", "pi"] {
        let context = agent_guard_rust::Context {
            consumer: match consumer {
                "claude" => adapters::Consumer::Claude,
                "codex" => adapters::Consumer::Codex,
                _ => adapters::Consumer::Pi,
            },
            home: "/synthetic/home".into(),
            cwd: "/synthetic/home/project".into(),
            user: Some("fixture-user".into()),
            zsh_executor: consumer != "pi",
            require_execution_owner: false,
            shell_observation_entries: std::cell::Cell::new(0),
        };
        for row in rows {
            let mut event = if let Some(source) = row["source"].as_str() {
                json!({"tool_name":if consumer=="pi" {"bash"} else {"Bash"},"tool_input":{"command":source}})
            } else {
                row["event"].clone()
            };
            if consumer == "pi" && event["tool_name"] == "Bash" {
                event["tool_name"] = json!("bash");
            }
            let bytes = serde_json::to_vec(&event).unwrap();
            let result = evaluate_with_arm(
                Event {
                    bytes: &bytes,
                    context: &context,
                    probe: &mut NoIoProbe,
                },
                Arm::Brush,
            );
            let wire = adapters::render(context.consumer, &result);
            println!(
                "{}",
                json!({"manifest_observation":true,"id":row["id"],"consumer":consumer,"class":support::class(&result),"evaluation":format!("{result:?}"),"exit":wire.exit,"stdout":wire.stdout,"stderr":wire.stderr})
            );
        }
    }
}
