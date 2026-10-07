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

#[derive(Default)]
struct Oracle {
    token_length_mismatches: Vec<Value>,
    substitution_end_limits: Vec<Value>,
    disagreements: Vec<Value>,
    span_disagreements: Vec<Value>,
    word_refusals: Vec<Value>,
    nested_refusals: Vec<Value>,
    compared: usize,
    words: usize,
    nested_scripts: usize,
    compared_inputs: BTreeSet<String>,
}

impl Oracle {
    fn compare_pieces(
        &mut self,
        raw: &str,
        pieces: &[WordPieceWithSource],
        parent: Quote,
        lexical: &Lexed<'_>,
        base: usize,
        id: &str,
    ) {
        let mut covered = 0;
        for p in pieces {
            if p.end_index <= covered {
                continue;
            }
            let mode = match &p.piece {
                WordPiece::SingleQuotedText(_) => Quote::Single,
                WordPiece::AnsiCQuotedText(_) => Quote::AnsiC,
                WordPiece::DoubleQuotedSequence(_) => Quote::Double,
                WordPiece::GettextDoubleQuotedSequence(_) => Quote::Gettext,
                _ => parent,
            };
            let range = p.start_index.max(covered)..p.end_index;
            assert!(
                raw.get(range.clone()).is_some(),
                "Brush piece offsets: {id}"
            );
            let opaque = matches!(
                &p.piece,
                WordPiece::CommandSubstitution(_)
                    | WordPiece::BackquotedCommandSubstitution(_)
                    | WordPiece::ParameterExpansion(_)
                    | WordPiece::ArithmeticExpression(_)
            );
            let sequence = matches!(
                &p.piece,
                WordPiece::DoubleQuotedSequence(_) | WordPiece::GettextDoubleQuotedSequence(_)
            );
            let mismatched_end = if matches!(
                p.piece,
                WordPiece::CommandSubstitution(_) | WordPiece::BackquotedCommandSubstitution(_)
            ) {
                lexical
                    .substitution_body(base + p.start_index)
                    .filter(|body| body.end + 1 != base + p.end_index)
            } else {
                None
            };
            if let Some(body) = &mismatched_end {
                self.substitution_end_limits.push(json!({"id":id,"word":raw,"brush_span":[base+p.start_index,base+p.end_index],"lexer_body":[body.start,body.end],"disposition":"Brush word substitution ends before the lexer region; production forwards the original lexical body (ruling 30)"}));
                covered = body.end + 1 - base;
            }
            for i in range.clone() {
                if mismatched_end.is_some() && i + 1 == range.end {
                    continue;
                }
                if (opaque || sequence) && i != range.start && i + 1 != range.end {
                    continue;
                }
                let context = lexical.context(base + i);
                self.compared += 1;
                self.compared_inputs.insert(id.to_owned());
                let escaped = matches!(&p.piece, WordPiece::EscapeSequence(_));
                if context.quote != mode || (escaped && !context.escaped) {
                    self.disagreements.push(json!({"id":id,"token":raw,"byte":base+i,"brush_quote":format!("{mode:?}"),"lexer":format!("{context:?}"),"piece":format!("{:?}",p.piece)}));
                }
            }
            match &p.piece {
                WordPiece::DoubleQuotedSequence(inner)
                | WordPiece::GettextDoubleQuotedSequence(inner) => {
                    self.compare_pieces(raw, inner, mode, lexical, base, id)
                }
                WordPiece::CommandSubstitution(_) | WordPiece::BackquotedCommandSubstitution(_) => {
                    let width = if matches!(&p.piece, WordPiece::BackquotedCommandSubstitution(_)) {
                        1
                    } else {
                        2
                    };
                    let body = &raw[range.start + width..range.end - 1];
                    let nested_base = base + range.start + width;
                    self.nested_scripts += 1;
                    let tokens = match brush_parser::uncached_tokenize_str(
                        body,
                        &brush_parser::TokenizerOptions::default(),
                    ) {
                        Ok(tokens) => tokens,
                        Err(error) => {
                            self.nested_refusals
                                .push(json!({"id":id,"body":body,"error":error.to_string()}));
                            continue;
                        }
                    };
                    let program = brush_parser::Parser::builder()
                        .build(std::io::Cursor::new(body.as_bytes()))
                        .parse_program();
                    self.compare_tokens(
                        body,
                        tokens,
                        program.ok().as_ref(),
                        lexical,
                        nested_base,
                        id,
                    );
                }
                _ => {}
            }
        }
    }

    fn compare_tokens(
        &mut self,
        source: &str,
        tokens: Vec<brush_parser::Token>,
        program: Option<&brush_parser::ast::Program>,
        lexical: &Lexed<'_>,
        base: usize,
        id: &str,
    ) {
        let offsets: Vec<_> = source
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(source.len()))
            .collect();
        let heredocs = program
            .map(|p| brush_heredocs(p, &offsets, source))
            .unwrap_or_default();
        for token in tokens {
            let brush_parser::Token::Word(raw, span) = token else {
                continue;
            };
            let local = offsets[span.start.index];
            let mut raw = raw.trim_start();
            let mut local = local + source[local..].len() - source[local..].trim_start().len();
            while let Some(tail) = source[local..].strip_prefix("\\\n") {
                local += 2;
                local += tail.len() - tail.trim_start().len();
            }
            // Brush emits a synthetic end-tag token after consuming its source bytes.
            // The independent AST body boundary and exact tag bytes bind its real location.
            if let Some(tag) = heredocs.iter().find_map(|(span, _)| {
                let tag = span.end + source[span.end..].len()
                    - source[span.end..].trim_start_matches('\t').len();
                let after = tag + raw.len();
                ((local == after
                    || (local == after + 1 && source.as_bytes().get(after) == Some(&b'\n')))
                    && source.get(tag..after) == Some(raw))
                .then_some(tag)
            }) {
                self.span_disagreements.push(json!({"id":id,"token":raw,"reported_byte":base+local,"actual_byte":base+tag,"disposition":"Brush synthetic heredoc end-tag span; mapped by independent AST body and exact original tag bytes"}));
                local = tag;
                for byte in local..local + raw.len() {
                    let context = lexical.context(base + byte);
                    if !context.heredoc_delimiter || context.active() {
                        self.disagreements.push(json!({"id":id,"byte":base+byte,"brush_end_tag":raw,"lexer":format!("{context:?}")}));
                    }
                }
            }
            let actual_end = offsets[span.end.index];
            if let Some(actual) = source.get(local..actual_end)
                && raw.len() != actual_end - local
            {
                self.token_length_mismatches.push(json!({"id":id,"byte":base+local,"brush_token":raw,"original_span":actual,"brush_length":raw.len(),"original_length":actual_end-local}));
                raw = actual;
            }
            for byte in local..(local + raw.len()).min(source.len()) {
                let context = lexical.context(base + byte);
                if context.comment
                    && context.command_depth == lexical.context(base + local).command_depth
                    && context.backtick_depth == lexical.context(base + local).backtick_depth
                {
                    self.disagreements.push(json!({"id":id,"byte":base+byte,"dimension":"word_comment","token":raw,"lexer":format!("{context:?}")}));
                }
            }
            let heredoc = heredocs.iter().find(|(span, _)| span.contains(&local));
            if let Some((span, expands)) = heredoc {
                let parent = lexical.context(base + span.start);
                for byte in span.clone() {
                    let context = lexical.context(base + byte);
                    let nested = *expands
                        && (context.command_depth > parent.command_depth
                            || context.parameter_depth > parent.parameter_depth
                            || context.backtick_depth > parent.backtick_depth
                            || context.arithmetic_depth > parent.arithmetic_depth);
                    if !nested && context.heredoc != Some(!expands) {
                        self.disagreements.push(json!({"id":id,"byte":base+byte,"brush_heredoc_expands":expands,"lexer":format!("{context:?}")}));
                    }
                }
                if !expands {
                    continue;
                }
            }
            let parsed = if heredoc.is_some() {
                word::parse_heredoc(raw, &brush_parser::ParserOptions::default())
            } else {
                word::parse(raw, &brush_parser::ParserOptions::default())
            };
            let pieces = match parsed {
                Ok(pieces) => pieces,
                Err(error) => {
                    self.word_refusals.push(json!({"id":id,"token":raw,"program_parsed":program.is_some(),"error":error.to_string()}));
                    continue;
                }
            };
            self.words += 1;
            self.compare_pieces(raw, &pieces, Quote::Unquoted, lexical, base + local, id);
        }
    }
}

fn brush_heredocs(
    program: &brush_parser::ast::Program,
    offsets: &[usize],
    source: &str,
) -> Vec<(std::ops::Range<usize>, bool)> {
    use brush_parser::ast::*;
    fn redirect(r: &IoRedirect, out: &mut Vec<(SourceSpan, bool, String, bool)>) {
        match r {
            IoRedirect::HereDocument(_, doc) => {
                if let Some(span) = &doc.doc.loc {
                    out.push((
                        span.clone(),
                        doc.requires_expansion,
                        doc.doc.value.clone(),
                        doc.remove_tabs,
                    ));
                }
            }
            IoRedirect::File(_, _, IoFileRedirectTarget::ProcessSubstitution(_, group)) => {
                list(&group.list, out)
            }
            _ => {}
        }
    }
    fn list(group: &CompoundList, out: &mut Vec<(SourceSpan, bool, String, bool)>) {
        for item in &group.0 {
            for (_, pipeline) in &item.0 {
                for c in &pipeline.seq {
                    command(c, out);
                }
            }
        }
    }
    fn compound(c: &CompoundCommand, out: &mut Vec<(SourceSpan, bool, String, bool)>) {
        match c {
            CompoundCommand::BraceGroup(g) => list(&g.list, out),
            CompoundCommand::Subshell(g) => list(&g.list, out),
            CompoundCommand::ForClause(g) => list(&g.body.list, out),
            CompoundCommand::ArithmeticForClause(g) => list(&g.body.list, out),
            CompoundCommand::IfClause(g) => {
                list(&g.condition, out);
                list(&g.then, out);
                if let Some(branches) = &g.elses {
                    for b in branches {
                        if let Some(c) = &b.condition {
                            list(c, out);
                        }
                        list(&b.body, out);
                    }
                }
            }
            CompoundCommand::WhileClause(g) | CompoundCommand::UntilClause(g) => {
                list(&g.0, out);
                list(&g.1.list, out);
            }
            CompoundCommand::CaseClause(g) => {
                for c in &g.cases {
                    if let Some(body) = &c.cmd {
                        list(body, out);
                    }
                }
            }
            CompoundCommand::Coprocess(g) => command(&g.body, out),
            CompoundCommand::Arithmetic(_) => {}
        }
    }
    fn redirects(rs: Option<&RedirectList>, out: &mut Vec<(SourceSpan, bool, String, bool)>) {
        if let Some(rs) = rs {
            for r in &rs.0 {
                redirect(r, out);
            }
        }
    }
    fn command(c: &Command, out: &mut Vec<(SourceSpan, bool, String, bool)>) {
        match c {
            Command::Simple(c) => {
                let items = c
                    .prefix
                    .iter()
                    .flat_map(|p| &p.0)
                    .chain(c.suffix.iter().flat_map(|p| &p.0));
                for item in items {
                    match item {
                        CommandPrefixOrSuffixItem::IoRedirect(r) => redirect(r, out),
                        CommandPrefixOrSuffixItem::ProcessSubstitution(_, g) => list(&g.list, out),
                        _ => {}
                    }
                }
            }
            Command::Compound(c, r) => {
                compound(c, out);
                redirects(r.as_ref(), out);
            }
            Command::Function(c) => {
                compound(&c.body.0, out);
                redirects(c.body.1.as_ref(), out);
            }
            Command::ExtendedTest(_, r) => redirects(r.as_ref(), out),
        }
    }
    let mut result = Vec::new();
    for c in &program.complete_commands {
        list(c, &mut result);
    }
    result
        .into_iter()
        .map(|(span, expands, value, remove_tabs)| {
            let start = offsets[span.start.index];
            let mut end = start;
            // Brush's doc span includes the end tag; its value owns the body boundary.
            for expected in value.split_inclusive('\n') {
                let next = source[end..]
                    .find('\n')
                    .map_or(source.len(), |n| end + n + 1);
                let actual = &source[end..next];
                assert_eq!(
                    if remove_tabs {
                        actual.trim_start_matches('\t')
                    } else {
                        actual
                    },
                    expected
                );
                end = next;
            }
            (start..end, expands)
        })
        .collect()
}

#[test]
fn lexer_matches_brush_word_quoting() {
    let inputs: Vec<Value> = include_str!("../fixtures/rust-parser-inputs.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|row| row["source"].is_string())
        .collect();
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
        serde_json::from_str(include_str!("../fixtures/rust-m1-5-oracle.json")).unwrap();
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

#[test]
fn escaped_continuation_token_mapping() {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("../fixtures/rust-m1-4-oracle.json")).unwrap();
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
