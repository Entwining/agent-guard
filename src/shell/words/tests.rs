use super::*;
use crate::shell::{Arm, lexer::Lexed};
use serde_json::{Value, json};

fn fragments(pieces: &[WordPieceWithSource], entries: &mut Vec<Value>) {
    for piece in pieces {
        match &piece.piece {
            WordPiece::ParameterExpansion(expr) => {
                for fragment in parameter_fragments(expr) {
                    let lexical = Lexed::parameter_fragment(
                        fragment,
                        crate::shell::lexer::Context::default(),
                    );
                    entries.push(json!({
                        "expr":format!("{expr:?}"),
                        "fragment":fragment,
                        "lexer_error":lexical.1.map(|error| format!("{error:?}"))
                    }));
                    let inner = word::parse(fragment, &ParserOptions::default()).unwrap();
                    fragments(&inner, entries);
                }
            }
            WordPiece::ArithmeticExpression(expr) => {
                let lexical =
                    Lexed::parameter_fragment(&expr.value, crate::shell::lexer::Context::default());
                entries.push(json!({
                    "expr":format!("{expr:?}"),
                    "fragment":expr.value,
                    "lexer_error":lexical.1.map(|error| format!("{error:?}"))
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
            Lexed::parameter_fragment(fragment, crate::shell::lexer::Context::default()),
            (_, Some(crate::shell::lexer::LexError::Unterminated { .. }))
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

#[test]
fn genuine_piece_fault_is_f_and_retains_independent_code() {
    let raw = "$(cat .env)public";
    let mut pieces = word::parse(raw, &ParserOptions::default()).unwrap();
    pieces.last_mut().unwrap().end_index = raw.len() + 1;
    let lexical = Lexed::scan(raw).unwrap();
    let mut out = Expanded {
        word: Word::literal(String::new()),
        split: Vec::new(),
        nested: Vec::new(),
        parameters: Vec::new(),
        unsupported: false,
    };
    let error = fill(
        raw,
        &pieces,
        &lexical,
        &BTreeMap::new(),
        crate::record::HostFacts {
            home: "/synthetic/home",
            user: None,
        },
        &mut out,
        &mut false,
    )
    .unwrap_err();
    assert_eq!(
        error.kind,
        CheckErrorKind::GuardFault,
        "genuine bad piece bounds must stay a fault"
    );
    assert_eq!(
        out.nested,
        ["cat .env"],
        "independently observed code survives the operational fault"
    );
    let result: Result<crate::Evaluation, CheckError> = Err(error);
    let wire = crate::adapters::render(crate::adapters::Consumer::Claude, &result);
    assert_eq!(wire.exit, 2);
    assert!(wire.stderr.contains("checker failed"));
}

#[test]
fn brush_accepted_word_lexer_refusal_is_unsupported() {
    let raw = "`echo $'broken`";
    assert!(word::parse(raw, &ParserOptions::default()).is_ok());
    assert!(matches!(
        Lexed::scan(raw),
        Err(crate::shell::lexer::LexError::Unterminated { .. })
    ));
    let expanded = expand(
        raw,
        &crate::shell::WordSyntax::Shell,
        &BTreeMap::new(),
        crate::record::HostFacts {
            home: "/synthetic/home",
            user: None,
        },
    )
    .unwrap();
    assert!(
        expanded.unsupported,
        "a known word parser disagreement is unsupported, not a checker fault"
    );
}

#[test]
fn outer_early_closer_has_its_own_unsupported_region() {
    let raw = "${v:-${w@Z}}";
    let expanded = expand(
        raw,
        &crate::shell::WordSyntax::Shell,
        &BTreeMap::new(),
        crate::record::HostFacts {
            home: "/synthetic/home",
            user: None,
        },
    )
    .unwrap();
    assert_eq!(
        expanded
            .parameters
            .iter()
            .map(|r| (r.range.clone(), r.supported))
            .collect::<Vec<_>>(),
        vec![(0..12, false), (5..11, false)]
    );
}

#[test]
fn arithmetic_piece_end_is_checked_against_lexer() {
    let raw = "$((1))";
    let mut pieces = word::parse(raw, &ParserOptions::default()).unwrap();
    pieces[0].end_index -= 1;
    let lexical = Lexed::scan(raw).unwrap();
    let mut out = Expanded {
        word: Word::literal(String::new()),
        split: Vec::new(),
        nested: Vec::new(),
        parameters: Vec::new(),
        unsupported: false,
    };
    fill(
        raw,
        &pieces,
        &lexical,
        &BTreeMap::new(),
        crate::record::HostFacts {
            home: "/synthetic/home",
            user: None,
        },
        &mut out,
        &mut false,
    )
    .unwrap();
    assert!(
        out.unsupported,
        "an in-bounds Brush end disagreement is unsupported"
    );
}
