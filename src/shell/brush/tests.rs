use super::*;

#[test]
fn explicit_empty_for_list_is_not_positional_arguments() {
    let raw = "for n in; do echo public; done";
    let parsed = records(raw, raw).unwrap().records.unwrap();
    assert!(matches!(&parsed[0], Statement::Loop { empty: true, .. }));
    let raw = "for n; do echo public; done";
    let parsed = records(raw, raw).unwrap().records.unwrap();
    assert!(matches!(&parsed[0], Statement::Loop { empty: false, .. }));
}

#[test]
fn missing_subscript_closer_is_unsupported_not_fault() {
    // Same-width original/AST disagreement injects the lexical missing closer
    // at the production assignment owner without changing its fault channel.
    let parsed = records("a[1 =public", "a[1]=public").unwrap();
    assert!(
        parsed
            .records
            .unwrap()
            .iter()
            .flat_map(|record| match record {
                Statement::Group(body) => body.as_slice(),
                other => std::slice::from_ref(other),
            })
            .any(|record| { matches!(record, Statement::UnsupportedSyntax) })
    );
}

#[test]
fn arithmetic_for_forwards_one_original_header_region() {
    let raw = "for ((i=0; i<1; i++)); do echo public; done";
    let parsed = records(raw, raw).unwrap().records.unwrap();
    let bodies: Vec<_> = parsed
        .iter()
        .flat_map(|record| match record {
            Statement::Group(body) => body.as_slice(),
            other => std::slice::from_ref(other),
        })
        .filter_map(|record| match record {
            Statement::Expansion(word) => Some(word.raw.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(bodies, ["i=0; i<1; i++"]);
}

#[test]
fn heredoc_text_mismatch_is_refused_and_original_body_is_forwarded() {
    let raw = "cat <<TAG\npublic\nTAG";
    let mut program = brush_parser::Parser::builder()
        .build(std::io::Cursor::new(raw.as_bytes()))
        .parse_program()
        .unwrap();
    let command = &mut program.complete_commands[0].0[0].0.first.seq[0];
    let Command::Simple(simple) = command else {
        panic!("simple command expected")
    };
    let mut changed = false;
    for item in simple
        .prefix
        .iter_mut()
        .flat_map(|p| &mut p.0)
        .chain(simple.suffix.iter_mut().flat_map(|p| &mut p.0))
    {
        if let CommandPrefixOrSuffixItem::IoRedirect(IoRedirect::HereDocument(_, doc)) = item {
            doc.doc.value = "changed parser text\n".into();
            changed = true;
        }
    }
    assert!(changed);
    let source = Source {
        text: raw,
        expansions: Vec::new(),
        offsets: raw
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(raw.len()))
            .collect(),
        lexical: super::super::lexer::Lexed::scan(raw).unwrap(),
    };
    let mut output = Vec::new();
    walk_command(&source, command, &mut output, None).unwrap();
    assert!(
        output
            .iter()
            .any(|r| matches!(r, Statement::UnsupportedSyntax))
    );
    assert!(
        output
            .iter()
            .any(|r| matches!(r, Statement::Expansion(word) if word.raw == "public\n"))
    );
}
