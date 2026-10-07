use crate::support;
use brush_parser::{SourceSpan, ast::*};
use serde_json::{Value, json};
use std::{collections::BTreeSet, io::Cursor};

#[derive(Default)]
struct Spans {
    statements: BTreeSet<(usize, usize)>,
    words: BTreeSet<(usize, usize)>,
}
fn range(source: &str, span: &SourceSpan) -> (usize, usize) {
    let offset = |index| {
        source
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(source.len()))
            .nth(index)
            .unwrap()
    };
    let (start, end) = (offset(span.start.index), offset(span.end.index));
    assert!(source.get(start..end).is_some());
    (start, end)
}
fn word(source: &str, word: &Word, spans: &mut Spans) {
    if let Some(loc) = &word.loc {
        spans.words.insert(range(source, loc));
    }
}
fn items(source: &str, items: &[CommandPrefixOrSuffixItem], spans: &mut Spans) {
    for item in items {
        match item {
            CommandPrefixOrSuffixItem::Word(w)
            | CommandPrefixOrSuffixItem::AssignmentWord(_, w) => word(source, w, spans),
            CommandPrefixOrSuffixItem::IoRedirect(r) => redirect(source, r, spans),
            CommandPrefixOrSuffixItem::ProcessSubstitution(_, g) => list(source, &g.list, spans),
        }
    }
}
fn redirect(source: &str, redirect: &IoRedirect, spans: &mut Spans) {
    match redirect {
        IoRedirect::File(
            _,
            _,
            IoFileRedirectTarget::Filename(w) | IoFileRedirectTarget::Duplicate(w),
        )
        | IoRedirect::HereString(_, w)
        | IoRedirect::OutputAndError(w, _) => word(source, w, spans),
        IoRedirect::File(_, _, IoFileRedirectTarget::ProcessSubstitution(_, g)) => {
            list(source, &g.list, spans)
        }
        IoRedirect::File(_, _, IoFileRedirectTarget::Fd(_)) => {}
        IoRedirect::HereDocument(_, doc) => word(source, &doc.here_end, spans),
    }
}
fn redirects(source: &str, redirects: Option<&RedirectList>, spans: &mut Spans) {
    if let Some(rs) = redirects {
        for r in &rs.0 {
            redirect(source, r, spans);
        }
    }
}
fn command(source: &str, command: &Command, spans: &mut Spans) {
    if let Some(loc) = command.location() {
        spans.statements.insert(range(source, &loc));
    }
    match command {
        Command::Simple(c) => {
            if let Some(prefix) = &c.prefix {
                items(source, &prefix.0, spans);
            }
            if let Some(w) = &c.word_or_name {
                word(source, w, spans);
            }
            if let Some(suffix) = &c.suffix {
                items(source, &suffix.0, spans);
            }
        }
        Command::Compound(c, rs) => {
            compound(source, c, spans);
            redirects(source, rs.as_ref(), spans);
        }
        Command::Function(f) => {
            compound(source, &f.body.0, spans);
            redirects(source, f.body.1.as_ref(), spans);
        }
        Command::ExtendedTest(_, rs) => redirects(source, rs.as_ref(), spans),
    }
}
fn list(source: &str, list: &CompoundList, spans: &mut Spans) {
    for item in &list.0 {
        for (_, pipeline) in &item.0 {
            for c in &pipeline.seq {
                command(source, c, spans);
            }
        }
    }
}
fn compound(source: &str, node: &CompoundCommand, spans: &mut Spans) {
    match node {
        CompoundCommand::BraceGroup(g) => list(source, &g.list, spans),
        CompoundCommand::Subshell(g) => list(source, &g.list, spans),
        CompoundCommand::ForClause(g) => {
            if let Some(values) = &g.values {
                for w in values {
                    word(source, w, spans);
                }
            }
            list(source, &g.body.list, spans);
        }
        CompoundCommand::ArithmeticForClause(g) => list(source, &g.body.list, spans),
        CompoundCommand::IfClause(g) => {
            list(source, &g.condition, spans);
            list(source, &g.then, spans);
            if let Some(branches) = &g.elses {
                for b in branches {
                    if let Some(c) = &b.condition {
                        list(source, c, spans);
                    }
                    list(source, &b.body, spans);
                }
            }
        }
        CompoundCommand::WhileClause(g) | CompoundCommand::UntilClause(g) => {
            list(source, &g.0, spans);
            list(source, &g.1.list, spans);
        }
        CompoundCommand::CaseClause(g) => {
            word(source, &g.value, spans);
            for c in &g.cases {
                for w in &c.patterns {
                    word(source, w, spans);
                }
                if let Some(c) = &c.cmd {
                    list(source, c, spans);
                }
            }
        }
        CompoundCommand::Coprocess(g) => command(source, &g.body, spans),
        CompoundCommand::Arithmetic(_) => {}
    }
}
fn tree_spans(source: &str, node: tree_sitter::Node<'_>, spans: &mut Spans) {
    assert!(source.get(node.byte_range()).is_some());
    if matches!(
        node.kind(),
        "command"
            | "declaration_command"
            | "test_command"
            | "for_statement"
            | "if_statement"
            | "while_statement"
            | "case_statement"
            | "subshell"
            | "compound_statement"
            | "function_definition"
    ) {
        spans
            .statements
            .insert((node.start_byte(), node.end_byte()));
    }
    let word_kind = |kind| {
        matches!(
            kind,
            "word"
                | "string"
                | "raw_string"
                | "ansi_c_string"
                | "concatenation"
                | "translated_string"
                | "simple_expansion"
                | "expansion"
                | "command_substitution"
        )
    };
    if word_kind(node.kind()) && !node.parent().is_some_and(|p| word_kind(p.kind())) {
        spans.words.insert((node.start_byte(), node.end_byte()));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        tree_spans(source, child, spans);
    }
}
fn compare(id: &str, source: &str) -> Value {
    let brush = brush_parser::Parser::builder()
        .build(Cursor::new(source))
        .parse_program();
    let mut b = Spans::default();
    if let Ok(program) = &brush {
        for c in &program.complete_commands {
            list(source, c, &mut b);
        }
    }
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let success = !tree.root_node().has_error();
    let mut t = Spans::default();
    tree_spans(source, tree.root_node(), &mut t);
    json!({"id":id,"comparison_owner":"parse-only","review_lead":brush.is_ok()!=success || b.statements!=t.statements || b.words!=t.words,"brush":{"success":brush.is_ok(),"statements":b.statements,"words":b.words},"tree":{"success":success,"statements":t.statements,"words":t.words}})
}

#[test]
fn parser_corpus_preserves_spans_and_frozen_agreement() {
    let fixture = support::Fixture::new();
    let mut inputs = Vec::new();
    let legacy: Vec<Value> = include_str!("../fixtures/contract.jsonl")
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    for row in &legacy {
        let id = format!("{}[{}]", row["family"].as_str().unwrap(), row["index"]);
        inputs.push((
            id,
            if row["tool"] == "Bash" {
                Ok(fixture.expand(row["input"].as_str().unwrap()))
            } else {
                Err("non_shell")
            },
        ));
    }
    let overlay: Vec<Value> = include_str!("../fixtures/rust-contract-classification.jsonl")
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    for row in overlay.iter().filter(|r| r["kind"] == "filesystem_link") {
        inputs.push((
            row["id"].as_str().unwrap().to_owned(),
            Err("setup_metadata"),
        ));
    }
    for row in support::rows()
        .iter()
        .filter(|r| support::is_evaluator_row(r))
    {
        let body = fixture.body(row);
        let event = agent_guard_rust::adapters::decode(
            fixture.context(row).consumer,
            &body,
            &fixture.project,
        );
        let id = format!("dev:{}", row["id"].as_str().unwrap());
        inputs.push((
            id,
            match event {
                Ok(e) => match e.operation {
                    agent_guard_rust::adapters::Operation::Shell(source) => Ok(source),
                    _ => Err("non_shell"),
                },
                Err(_) => Err("malformed_event"),
            },
        ));
    }
    for (i, source) in [
        "cat $'\\u002eenv'",
        "cat ~/.e{n..n}v",
        "cat '~/.e{n..n}v'",
        "ls ~fixture-user/Library/Containers",
        "cat $(pwd -L)/x",
        "cat `pwd -P`/x",
        "echo \"${v:-$API_KEY}\"",
        "cat public\\ file",
        "cat \"\\\n.env\"",
        "cat <<'EOF'\nliteral 'quote\nEOF",
        "printf '中文'",
        "cat <&0",
        "if then",
    ]
    .iter()
    .enumerate()
    {
        inputs.push((format!("variant:{i}"), Ok(source.to_string())));
    }
    let expected_ids: BTreeSet<_> = inputs.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(expected_ids.len(), inputs.len(), "duplicate parser input");
    let report: Vec<_> = inputs
        .iter()
        .map(|(id, source)| match source {
            Ok(source) => compare(id, source),
            Err(status) => json!({"id":id,"comparison_owner":"parse-only","status":status}),
        })
        .collect();
    assert_eq!(
        report
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect::<BTreeSet<_>>(),
        expected_ids,
        "every parser input must be evaluated"
    );
    for row in &report {
        assert!(row.get("class").is_none() && row.get("observations").is_none());
    }
    assert!(report.iter().any(|row| row["brush"]["success"] == true));
    assert!(report.iter().any(|row| row["tree"]["success"] == false));
}
