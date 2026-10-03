use super::{Parsed, Record, Redirect, Word};
use crate::{CheckError, CheckErrorKind};
use tree_sitter::Node;

fn text<'a>(source: &'a str, node: Node<'_>) -> Result<&'a str, CheckError> {
    source.get(node.byte_range()).ok_or(CheckError {
        kind: CheckErrorKind::GuardFault,
    })
}

pub(super) fn records(source: &str, parsed: &str) -> Result<Parsed, CheckError> {
    // bash 0.25 treats # after an escaped newline as a comment even within an existing word.
    let mut bytes = parsed.as_bytes().to_vec();
    // The pinned grammar rejects <> and a final escaped newline. Keep byte offsets intact.
    for index in 0..bytes.len().saturating_sub(1) {
        if bytes[index..].starts_with(b"<>") {
            bytes[index + 1] = b' ';
        }
    }
    if bytes.ends_with(b"\\\n") {
        let length = bytes.len();
        bytes[length - 2..].fill(b' ');
    }
    for index in 1..bytes.len().saturating_sub(2) {
        if bytes[index] == b'\\'
            && bytes[index + 1] == b'\n'
            && bytes[index + 2] == b'#'
            && !bytes[index - 1].is_ascii_whitespace()
        {
            bytes[index] = b'_';
            bytes[index + 1] = b'_';
        }
    }
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .map_err(|_| CheckError {
            kind: CheckErrorKind::GuardFault,
        })?;
    let tree = parser.parse(&bytes, None).ok_or(CheckError {
        kind: CheckErrorKind::GuardFault,
    })?;
    let spans = vec![tree.root_node().byte_range()];
    text(source, tree.root_node())?;
    if tree.root_node().has_error() {
        return Ok(Parsed {
            records: None,
            spans,
        });
    }
    let mut output = Vec::new();
    walk(source, tree.root_node(), &mut output)?;
    if output.iter().any(|record|matches!(record,Record::Command {argv,..} if argv.first().is_some_and(|word|["then","fi","else","elif","do","done","esac","in"].contains(&word.raw.as_str())))) {
        return Ok(Parsed {records:None,spans});
    }
    Ok(Parsed {
        records: Some(output),
        spans,
    })
}

fn walk(source: &str, node: Node<'_>, output: &mut Vec<Record>) -> Result<(), CheckError> {
    match node.kind() {
        "for_statement" => {
            if let Some(variable) = node.child_by_field_name("variable") {
                let mut cursor = node.walk();
                let values = node
                    .children_by_field_name("value", &mut cursor)
                    .map(|value| {
                        text(source, value).map(|raw| Word {
                            raw: raw.to_owned(),
                        })
                    })
                    .collect::<Result<_, _>>()?;
                output.push(Record::LoopBinding(
                    text(source, variable)?.to_owned(),
                    values,
                ));
            }
            if let Some(body) = node.child_by_field_name("body") {
                walk(source, body, output)?;
            }
            return Ok(());
        }
        "declaration_command" => {
            let keyword = node.child(0).ok_or(CheckError {
                kind: CheckErrorKind::GuardFault,
            })?;
            let mut argv = vec![Word {
                raw: text(source, keyword)?.to_owned(),
            }];
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                argv.push(Word {
                    raw: text(source, child)?.to_owned(),
                });
            }
            output.push(Record::Command {
                argv,
                redirects: Vec::new(),
                pipeline: None,
            });
            return Ok(());
        }
        "function_definition" => {
            let name = node.child_by_field_name("name").ok_or(CheckError {
                kind: CheckErrorKind::GuardFault,
            })?;
            let body = node.child_by_field_name("body").ok_or(CheckError {
                kind: CheckErrorKind::GuardFault,
            })?;
            let mut records = Vec::new();
            walk(source, body, &mut records)?;
            output.push(Record::Definition(text(source, name)?.to_owned(), records));
            return Ok(());
        }
        "variable_assignment" => {
            if let Some((name, raw)) = text(source, node)?.split_once('=') {
                output.push(Record::Assignment(
                    name.to_owned(),
                    Word {
                        raw: raw.to_owned(),
                    },
                ));
            }
            return Ok(());
        }
        "command" => {
            let mut argv = Vec::new();
            let mut redirects = Vec::new();
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                match child.kind() {
                    "variable_assignment" => walk(source, child, output)?,
                    "file_redirect" => file_redirect(source, child, &mut redirects)?,
                    "herestring_redirect" => {
                        if let Some(value) = child.child_by_field_name("value") {
                            output.push(Record::Expansion(Word {
                                raw: text(source, value)?.to_owned(),
                            }));
                        }
                    }
                    _ => argv.push(Word {
                        raw: text(source, child)?.to_owned(),
                    }),
                }
            }
            let parent = node.parent().and_then(|p| {
                if p.kind() == "redirected_statement" {
                    p.parent()
                } else {
                    Some(p)
                }
            });
            let pipeline = parent
                .filter(|p| p.kind() == "pipeline")
                .map(|p| p.start_byte());
            output.push(Record::Command {
                argv,
                redirects,
                pipeline,
            });
            return Ok(());
        }
        "file_redirect" => {
            let mut redirects = Vec::new();
            file_redirect(source, node, &mut redirects)?;
            output.push(Record::Command {
                argv: Vec::new(),
                redirects,
                pipeline: None,
            });
            return Ok(());
        }
        "heredoc_redirect" => {
            let mut cursor = node.walk();
            let nodes: Vec<_> = node.named_children(&mut cursor).collect();
            let mut cursor = node.walk();
            for argument in node.children_by_field_name("argument", &mut cursor) {
                if let Some(Record::Command { argv, .. }) = output
                    .iter_mut()
                    .rev()
                    .find(|record| matches!(record, Record::Command { .. }))
                {
                    argv.push(Word {
                        raw: text(source, argument)?.to_owned(),
                    });
                }
            }
            let quoted = nodes
                .iter()
                .find(|n| n.kind() == "heredoc_start")
                .map(|n| text(source, *n))
                .transpose()?
                .is_some_and(|s| s.contains(['\'', '"', '\\']));
            if !quoted {
                for body in nodes.iter().filter(|n| n.kind() == "heredoc_body") {
                    output.push(Record::Expansion(Word {
                        raw: text(source, *body)?.to_owned(),
                    }));
                }
            }
            // bash 0.25 nests same-line commands here, alongside the inert body/delimiters.
            for child in nodes {
                if !matches!(
                    child.kind(),
                    "heredoc_start" | "heredoc_body" | "heredoc_end"
                ) {
                    walk(source, child, output)?;
                }
            }
            return Ok(());
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk(source, child, output)?;
    }
    Ok(())
}

fn file_redirect(
    source: &str,
    node: Node<'_>,
    redirects: &mut Vec<Redirect>,
) -> Result<(), CheckError> {
    let raw = text(source, node)?;
    let mut cursor = node.walk();
    for target in node.children_by_field_name("destination", &mut cursor) {
        redirects.push(Redirect {
            target: Word {
                raw: text(source, target)?.to_owned(),
            },
            write: raw.contains('>') && !raw.contains("<>"),
        });
    }
    Ok(())
}
