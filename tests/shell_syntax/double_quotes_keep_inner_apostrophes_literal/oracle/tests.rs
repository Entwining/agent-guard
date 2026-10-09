use super::*;

#[test]
fn lexer_matches_brush_word_quoting() {
    let inputs: Vec<Value> = include_str!("../../../fixtures/rust-parser-inputs.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|row| row["source"].is_string())
        .collect();
    assert!(!inputs.is_empty(), "missing shell-source parser partition");
    let mut oracle = Oracle::default();
    let mut tokenized = 0;
    let mut parsed_programs = 0;
    let mut skipped = BTreeSet::new();
    let mut known_limits = Vec::new();
    let mut evaluated = BTreeSet::new();
    for row in &inputs {
        let id = row["id"].as_str().unwrap();
        let source = row["source"].as_str().unwrap();
        assert!(evaluated.insert(id), "duplicate parser input: {id}");
        let Ok(tokens) =
            brush_parser::uncached_tokenize_str(source, &brush_parser::TokenizerOptions::default())
        else {
            skipped.insert(id);
            continue;
        };
        tokenized += 1;
        let program = brush_parser::Parser::builder()
            .build(std::io::Cursor::new(source.as_bytes()))
            .parse_program();
        if program.is_ok() {
            parsed_programs += 1;
        }
        let lexical = match Lexed::scan(source) {
            Ok(lexical) => lexical,
            Err(error) => {
                if row["lexer_refusal"] == true {
                    known_limits
                        .push(json!({"id":id,"source":source,"lexer_error":format!("{error:?}")}));
                    continue;
                }
                oracle
                    .disagreements
                    .push(json!({"id":id,"source":source,"lexer_error":format!("{error:?}")}));
                continue;
            }
        };
        oracle.compare_tokens(source, tokens, program.ok().as_ref(), &lexical, 0, id);
    }
    println!(
        "{}",
        json!({"oracle_inputs":inputs.len(),"tokenized":tokenized,"parsed_programs":parsed_programs,"word_parses":oracle.words,"nested_scripts":oracle.nested_scripts,"compared_bytes":oracle.compared,"tokenizer_refusals":skipped.len(),"tokenizer_refused_inputs":skipped,"known_limits":known_limits,"span_disagreements":oracle.span_disagreements,"token_length_mismatches":oracle.token_length_mismatches,"substitution_end_limits":oracle.substitution_end_limits,"word_refusals":oracle.word_refusals,"nested_refusals":oracle.nested_refusals,"disagreements":oracle.disagreements})
    );
    assert_eq!(
        evaluated,
        inputs
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect(),
        "every parser input must be evaluated"
    );
    assert!(oracle.nested_refusals.is_empty());
    assert!(tokenized > 0 && oracle.words > 0 && oracle.compared > 0);
    for id in ["00002", "00003"] {
        assert!(
            oracle.compared_inputs.contains(id),
            "uncompared named witness: {id}"
        );
    }
    assert!(
        oracle
            .word_refusals
            .iter()
            .all(|row| row["program_parsed"] == false)
    );
    assert!(
        oracle.disagreements.is_empty(),
        "lexer / Brush disagreement count: {}",
        oracle.disagreements.len()
    );
}

#[test]
fn token_length_mismatch_keeps_original_source_spans() {
    let sources: Vec<String> =
        serde_json::from_str(include_str!("../../../fixtures/rust-m1-5-oracle.json")).unwrap();
    assert!(!sources.is_empty());
    for (index, source) in sources.iter().enumerate() {
        let lexical = Lexed::scan(source).unwrap();
        let tokens =
            brush_parser::uncached_tokenize_str(source, &brush_parser::TokenizerOptions::default())
                .unwrap();
        let program = brush_parser::Parser::builder()
            .build(std::io::Cursor::new(source.as_bytes()))
            .parse_program()
            .ok();
        let mut oracle = Oracle::default();
        oracle.compare_tokens(
            source,
            tokens,
            program.as_ref(),
            &lexical,
            0,
            &format!("opus-{index}"),
        );
        assert!(
            !oracle.token_length_mismatches.is_empty(),
            "the original token span must compare: {source}"
        );
        assert!(
            oracle.compared > 0 && oracle.disagreements.is_empty(),
            "{source}: {:?}",
            oracle.disagreements
        );
        if !source.starts_with("echo $((") {
            assert!(
                !oracle.substitution_end_limits.is_empty(),
                "Brush's end limit must remain visible: {source}"
            );
        }
        println!(
            "{}",
            json!({"source":source,"token_length_mismatches":oracle.token_length_mismatches,"substitution_end_limits":oracle.substitution_end_limits,"compared_bytes":oracle.compared})
        );
    }
}

#[test]
fn escaped_continuation_token_mapping() {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("../../../fixtures/rust-m1-4-oracle.json")).unwrap();
    assert!(
        !rows.is_empty(),
        "missing escaped-continuation oracle partition"
    );
    for row in rows {
        let source = row["source"].as_str().unwrap();
        let tokens =
            brush_parser::uncached_tokenize_str(source, &brush_parser::TokenizerOptions::default())
                .unwrap();
        let lexical = Lexed::scan(source).unwrap();
        let mut oracle = Oracle::default();
        oracle.compare_tokens(source, tokens, None, &lexical, 0, "F2");
        assert!(
            oracle.compared > 0 && oracle.compared_inputs.contains("F2"),
            "continuation witness was not compared"
        );
        println!(
            "{}",
            json!({"id":"F2","disagreements":oracle.disagreements})
        );
        assert!(
            oracle.disagreements.is_empty(),
            "F2 normalized token start disagrees with source"
        );
    }
}
