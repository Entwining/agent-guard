use crate::support;
use agent_guard_rust::{
    Coverage, CoverageGap, Event, evaluate_with_arm,
    shell::{
        Arm,
        lexer::{LexError, Lexed, Quote},
    },
};
use brush_parser::SourceSpan;
use brush_parser::word::{self, WordPiece, WordPieceWithSource};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn regressions(owner: &str) {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("../fixtures/rust-m1-3-regressions.json")).unwrap();
    let fixture = support::Fixture::new();
    for consumer in ["claude", "codex", "pi"] {
        let context = fixture.context(&json!({"consumer":consumer,"cwd":"$P"}));
        let selected: Vec<_> = rows.iter().filter(|row| row["owner"] == owner).collect();
        assert!(!selected.is_empty(), "missing owner {owner}");
        for row in selected {
            let body = serde_json::to_vec(
                &json!({"tool_name":if consumer=="pi" {"bash"} else {"Bash"},
                "tool_input":{"command":fixture.expand(row["source"].as_str().unwrap())}}),
            )
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
            assert_eq!(
                support::class(&result),
                row["expected"],
                "{consumer}: {row}"
            );
            if row["expected"] == "UR" {
                let Coverage::LimitedPreflight(gaps) = &result.as_ref().unwrap().coverage else {
                    panic!("D1 coverage missing: {row}");
                };
                assert!(
                    gaps.contains(&if consumer == "pi" {
                        CoverageGap::UnsupportedDialectConstruct
                    } else {
                        CoverageGap::ExecutorDivergence
                    }),
                    "D1 must detect the active syntax, independently of parser refusal: {row}"
                );
            }
            if row["expected"] == "D" {
                let agent_guard_rust::Outcome::ProtectedDenial { reason, recovery } =
                    &result.as_ref().unwrap().outcome
                else {
                    panic!("missing protected denial: {row}");
                };
                assert!(
                    reason.effect.contains(row["protection"].as_str().unwrap()),
                    "{row}: {reason:?}"
                );
                assert!(!recovery.excluded_scope.is_empty());
            }
            let wire = agent_guard_rust::adapters::render(context.consumer, &result);
            assert_eq!(wire.exit, if row["expected"] == "N" { 0 } else { 2 });
            assert!(wire.stdout.is_empty());
        }
    }
}

#[test]
fn double_quotes_keep_inner_apostrophes_literal() {
    regressions("double_context");
}
#[test]
fn executable_qualifiers_survive_double_quoted_apostrophes() {
    regressions("qualifier");
}
#[test]
fn dialect_commands_survive_double_quoted_apostrophes() {
    regressions("command_gate");
}
#[test]
fn equals_process_substitution_extracts_nested_targets() {
    regressions("process");
    let row = json!({"consumer":"claude", "cwd":"$P", "event":{"tool_name":"Bash", "tool_input":{"command":"cat =(true) .env"}}});
    let fixture = support::Fixture::new();
    let context = fixture.context(&row);
    let bytes = serde_json::to_vec(&row["event"]).unwrap();
    let result = evaluate_with_arm(
        Event {
            bytes: &bytes,
            context: &context,
            probe: &mut support::RecordingProbe::literal_for_quoted_paths(&fixture),
        },
        Arm::Brush,
    )
    .unwrap();
    assert!(matches!(
        result.outcome,
        agent_guard_rust::Outcome::ProtectedDenial { .. }
    ));
    assert!(result.effects.iter().any(|e| matches!(
        e,
        agent_guard_rust::EffectRecord::ProtectedTarget {
            protection: agent_guard_rust::filesystem::Protection::Environment,
            write: false,
            ..
        }
    )));
}
#[test]
fn resolved_unquoted_bindings_keep_bash_glob_reach() {
    regressions("expansion_glob");
}
#[test]
fn literal_data_and_nested_code_keep_d1_masking() {
    regressions("masking");
}

#[test]
fn unterminated_quotes_are_blocked() {
    let fixture = support::Fixture::new();
    let context = fixture.context(&json!({"consumer":"claude","cwd":"$P"}));
    for source in [
        "printf 'x",
        "printf \"it's",
        "printf $'it\\'s",
        "printf $\"it's",
        "printf `echo x",
        "printf \"$(echo 'x')",
    ] {
        assert!(
            matches!(Lexed::scan(source), Err(LexError::Unterminated { .. })),
            "{source}"
        );
        let body = serde_json::to_vec(&json!({"tool_name":"Bash","tool_input":{"command":source}}))
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
        assert_eq!(support::class(&result), "UR", "{source}");
        let wire = agent_guard_rust::adapters::render(context.consumer, &result);
        assert_eq!(wire.exit, 2);
        assert!(wire.stdout.is_empty());
    }
}

#[path = "double_quotes_keep_inner_apostrophes_literal/oracle.rs"]
mod oracle;

#[test]
fn lexer_exposes_nested_and_heredoc_context() {
    let source = r#"printf "%s" "$(printf '%s' x)" "${v:-x}" `printf 'x'` "$((1<<2))""#;
    let lexical = Lexed::scan(source).unwrap();
    let command = source.find("$(printf").unwrap() + 2;
    assert_eq!(lexical.context(command).command_depth, 1);
    assert_eq!(lexical.context(command).quote, Quote::Unquoted);
    let parameter = source.find("${v").unwrap() + 2;
    assert_eq!(lexical.context(parameter).parameter_depth, 1);
    assert_eq!(lexical.context(parameter).quote, Quote::Double);
    let backtick = source.find("`printf").unwrap() + 1;
    assert_eq!(lexical.context(backtick).backtick_depth, 1);
    let arithmetic = source.find("<<").unwrap();
    assert_eq!(lexical.context(arithmetic).arithmetic_depth, 1);
    assert!(lexical.context(arithmetic).heredoc.is_none());
    for (source, quoted) in [
        ("cat <<'EOF'\n'\" ${(f)v}\nEOF", true),
        ("cat <<EOF\n'\" ${(f)v}\nEOF", false),
    ] {
        let lexical = Lexed::scan(source).unwrap();
        let body = source.find("${(").unwrap();
        assert_eq!(lexical.context(body).heredoc, Some(quoted));
        assert_eq!(lexical.context(body).quote, Quote::Unquoted);
        assert_eq!(lexical.context(body).active(), !quoted);
    }
    let source = "cat <<\"${(f)v}\"\nDATA\n${(f)v}";
    let lexical = Lexed::scan(source).unwrap();
    let delimiter = source.find("${(").unwrap();
    assert!(lexical.context(delimiter).heredoc_delimiter);
    assert!(!lexical.context(delimiter).active());
    let end_tag = source.rfind("${(").unwrap();
    assert!(lexical.context(end_tag).heredoc_delimiter);
    assert!(!lexical.context(end_tag).active());
    let source = r#"printf "\$x" # ${(f)v}"#;
    let lexical = Lexed::scan(source).unwrap();
    assert!(lexical.context(source.find('$').unwrap()).escaped);
    assert!(lexical.context(source.find("${(").unwrap()).comment);
}

#[test]
fn target_tilde_uses_lexical_prefix_context() {
    use agent_guard_rust::record::{Effect, HostFacts, Target, Walk, Word};
    let fixture = support::Fixture::new();
    for (raw, quoted) in [
        ("'~/Library/Containers'", true),
        ("\"~/Library/Containers\"", true),
        ("$'~/Library/Containers'", false),
        ("$\"~/Library/Containers\"", false),
        ("\\~/Library/Containers", false),
        ("~/Library/Containers", false),
    ] {
        let mut word = Word::literal("~/Library/Containers".into());
        word.raw = raw.into();
        let target = Target::from_word(
            &word,
            &fixture.project,
            HostFacts {
                home: &fixture.home,
                user: None,
            },
            Effect::Read,
            Walk::None,
        );
        assert_eq!(
            target.path,
            if quoted {
                format!("{}/~/Library/Containers", fixture.project)
            } else {
                format!("{}/Library/Containers", fixture.home)
            },
            "{raw}"
        );
    }
}
