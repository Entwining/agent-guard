use super::*;
use crate::shell::{Arm, lexer::Lexed};
use serde_json::{Value, json};

fn fragments(pieces: &[WordPieceWithSource], entries: &mut Vec<Value>) {
    for piece in pieces {
        match &piece.piece {
            WordPiece::ParameterExpansion(expr) => {
                for fragment in parameter_fragments(expr) {
                    let lexical = Lexed::parameter_fragment(fragment);
                    entries.push(json!({
                        "expr":format!("{expr:?}"),
                        "fragment":fragment,
                        "lexer_error":lexical.as_ref().err().map(|error| format!("{error:?}"))
                    }));
                    let inner = word::parse(fragment, &ParserOptions::default()).unwrap();
                    fragments(&inner, entries);
                }
            }
            WordPiece::ArithmeticExpression(expr) => {
                let lexical = Lexed::parameter_fragment(&expr.value);
                entries.push(json!({
                    "expr":format!("{expr:?}"),
                    "fragment":expr.value,
                    "lexer_error":lexical.as_ref().err().map(|error| format!("{error:?}"))
                }));
                let inner = word::parse(&expr.value, &ParserOptions::default()).unwrap();
                fragments(&inner, entries);
            }
            WordPiece::DoubleQuotedSequence(inner)
            | WordPiece::GettextDoubleQuotedSequence(inner) => fragments(inner, entries),
            _ => {}
        }
    }
}

#[test]
fn brush_production_fragments_preserve_refusal() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust-m1-4-fragments.json"
    ))
    .unwrap();
    let mut failures = Vec::new();
    let mut calls = 0;
    for row in &rows {
        let raw = row["word"].as_str().unwrap();
        let (input, _) = brace_text(raw).unwrap();
        let pieces = word::parse(&input, &ParserOptions::default()).unwrap();
        let mut entries = Vec::new();
        fragments(&pieces, &mut entries);
        calls += entries.len();
        let observation = crate::shell::observe(
            row["source"].as_str().unwrap(),
            Arm::Brush,
            "/synthetic/home",
            "/synthetic/home/project",
            true,
        );
        // A malformed Brush fragment may require refusal; it cannot erase that
        // refusal or turn a valid source's dialect limit into a checker fault.
        let accepted = match &observation {
            Ok(observation) if row["expected"] == "UR" => observation
                .gaps
                .contains(&crate::CoverageGap::ExecutorDivergence),
            Ok(observation) => observation.gaps.is_empty(),
            Err(_) => false,
        };
        println!(
            "{}",
            json!({"id":row["id"],"source":row["source"],"fragments":entries,"observation":format!("{observation:?}"),"accepted":accepted})
        );
        if !accepted {
            failures.push(row["id"].clone());
        }
    }
    assert!(calls > 0, "production fragment entry was not exercised");
    assert!(
        failures.is_empty(),
        "fragment oracle failures: {failures:?}"
    );
}

#[test]
fn unterminated_parameter_fragments_are_errors() {
    for fragment in ["${(f)v", "a${(f)v", "${w:-${(f)v", "${w"] {
        assert!(matches!(
            Lexed::parameter_fragment(fragment),
            Err(crate::shell::lexer::LexError::Unterminated { .. })
        ));
    }
}

#[test]
fn escaped_dollar_brace_reach_queries_owner() {
    let raw = r"\${.env,x}";
    let lexical = Lexed::scan(raw).unwrap();
    assert!(lexical.context(1).escaped);
    let (_, reach) = brace_text(raw).unwrap();
    assert!(reach, "brace expansion follows an escaped literal dollar");
}
