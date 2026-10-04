use super::{Parsed, RawRedirect as Redirect, RawWord as Word, Record};
use std::ops::Range;

struct Source<'a> {
    text: &'a str,
    offsets: Vec<usize>,
    lexical: super::lexer::Lexed<'a>,
}

impl Source<'_> {
    fn range(&self, span: &SourceSpan) -> Result<Range<usize>, CheckError> {
        let start = self.offsets.get(span.start.index).copied();
        let end = self.offsets.get(span.end.index).copied();
        start
            .zip(end)
            .map(|(start, end)| start..end)
            .filter(|r| self.text.get(r.clone()).is_some())
            .ok_or(CheckError {
                kind: CheckErrorKind::GuardFault,
            })
    }
}
use crate::{CheckError, CheckErrorKind};
use brush_parser::{SourceSpan, ast::*};

fn word(source: &Source<'_>, parsed: &WordAst) -> Result<Word, CheckError> {
    let raw = if let Some(span) = &parsed.loc {
        original(source, span)?.to_owned()
    } else {
        parsed.value.clone()
    };
    // brush 0.4 spans can include separator whitespace after continuations and heredocs.
    let mut raw = raw.trim_start_matches(char::is_whitespace);
    while let Some(tail) = raw.strip_prefix("\\\n") {
        raw = tail.trim_start_matches(char::is_whitespace);
    }
    Ok(Word {
        raw: raw.to_owned(),
        syntax: super::WordSyntax::Shell,
    })
}
type WordAst = brush_parser::ast::Word;

fn original<'a>(source: &'a Source<'_>, span: &SourceSpan) -> Result<&'a str, CheckError> {
    source.text.get(source.range(span)?).ok_or(CheckError {
        kind: CheckErrorKind::GuardFault,
    })
}

pub(super) fn records(source: &str, parsed: &str) -> Result<Parsed, CheckError> {
    let (lexical, error) =
        super::lexer::Lexed::parameter_fragment(source, super::lexer::Context::default());
    if error == Some(super::lexer::LexError::Nesting) {
        return Err(CheckError {
            kind: CheckErrorKind::ResourceLimit,
        });
    }
    let source = Source {
        text: source,
        lexical,
        offsets: parsed
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(parsed.len()))
            .collect(),
    };
    let tokens =
        brush_parser::uncached_tokenize_str(parsed, &brush_parser::TokenizerOptions::default());
    let mut spans: Vec<_> = std::iter::once(0..source.text.len()).collect();
    if let Ok(tokens) = tokens {
        for token in tokens {
            spans.push(source.range(token.location())?);
        }
    }
    let Ok(program) = brush_parser::Parser::builder()
        .build(std::io::Cursor::new(parsed.as_bytes()))
        .parse_program()
    else {
        return Ok(Parsed {
            records: None,
            spans,
        });
    };
    let mut output = Vec::new();
    for list in &program.complete_commands {
        walk_list(&source, list, &mut output)?;
    }
    Ok(Parsed {
        records: Some(output),
        spans,
    })
}

fn walk_list(
    source: &Source<'_>,
    list: &CompoundList,
    output: &mut Vec<Record>,
) -> Result<(), CheckError> {
    for item in &list.0 {
        for (_, pipeline) in &item.0 {
            let group = if pipeline.seq.len() > 1 {
                pipeline
                    .location()
                    .map(|span| source.range(&span))
                    .transpose()?
                    .map(|span| span.start)
            } else {
                None
            };
            for command in &pipeline.seq {
                walk_command(source, command, output, group)?;
            }
        }
    }
    Ok(())
}

fn walk_command(
    source: &Source<'_>,
    command: &Command,
    output: &mut Vec<Record>,
    pipeline: Option<usize>,
) -> Result<(), CheckError> {
    match command {
        Command::Simple(simple) => {
            let mut argv = Vec::new();
            let mut redirects = Vec::new();
            if let Some(prefix) = &simple.prefix {
                for item in &prefix.0 {
                    item_record(source, item, &mut argv, &mut redirects, output)?;
                }
            }
            if let Some(name) = &simple.word_or_name {
                argv.push(word(source, name)?);
            }
            if let Some(suffix) = &simple.suffix {
                for item in &suffix.0 {
                    item_record(source, item, &mut argv, &mut redirects, output)?;
                }
            }
            if !argv.is_empty() || !redirects.is_empty() {
                output.push(Record::Command {
                    argv,
                    redirects,
                    pipeline,
                });
            }
        }
        Command::Compound(compound, redirects) => {
            walk_compound(source, compound, output)?;
            redirect_list(source, redirects.as_ref(), output)?;
        }
        Command::Function(function) => {
            let mut body = Vec::new();
            walk_compound(source, &function.body.0, &mut body)?;
            redirect_list(source, function.body.1.as_ref(), &mut body)?;
            output.push(Record::Definition(function.fname.value.clone(), body));
        }
        Command::ExtendedTest(test, redirects) => {
            output.push(Record::Expansion(Word {
                raw: original(source, &test.loc)?.to_owned(),
                syntax: super::WordSyntax::Shell,
            }));
            redirect_list(source, redirects.as_ref(), output)?;
        }
    }
    Ok(())
}

fn walk_compound(
    source: &Source<'_>,
    command: &CompoundCommand,
    output: &mut Vec<Record>,
) -> Result<(), CheckError> {
    match command {
        CompoundCommand::BraceGroup(group) => walk_list(source, &group.list, output)?,
        CompoundCommand::Subshell(group) => walk_list(source, &group.list, output)?,
        CompoundCommand::ForClause(group) => {
            if let Some(values) = &group.values {
                output.push(Record::LoopBinding(
                    group.variable_name.clone(),
                    values
                        .iter()
                        .map(|value| word(source, value))
                        .collect::<Result<_, _>>()?,
                ));
            }
            walk_list(source, &group.body.list, output)?;
        }
        CompoundCommand::ArithmeticForClause(group) => {
            let range = source.range(&group.loc)?;
            let header = original(source, &group.loc)?;
            if let Some(left) = header.find("((").map(|i| i + range.start)
                && let Some(right) = source.lexical.closing(left, b'(', b')')
                && right < range.end
            {
                output.push(Record::Expansion(Word {
                    raw: source.text[left + 2..right - 1].to_owned(),
                    syntax: super::WordSyntax::Arithmetic,
                }));
            } else {
                output.push(Record::UnsupportedSyntax);
            }
            walk_list(source, &group.body.list, output)?;
        }
        CompoundCommand::IfClause(group) => {
            walk_list(source, &group.condition, output)?;
            walk_list(source, &group.then, output)?;
            if let Some(elses) = &group.elses {
                for branch in elses {
                    if let Some(condition) = &branch.condition {
                        walk_list(source, condition, output)?;
                    }
                    walk_list(source, &branch.body, output)?;
                }
            }
        }
        CompoundCommand::WhileClause(group) | CompoundCommand::UntilClause(group) => {
            walk_list(source, &group.0, output)?;
            walk_list(source, &group.1.list, output)?;
        }
        CompoundCommand::CaseClause(group) => {
            output.push(Record::Expansion(word(source, &group.value)?));
            for case in &group.cases {
                for pattern in &case.patterns {
                    output.push(Record::Expansion(word(source, pattern)?));
                }
                if let Some(body) = &case.cmd {
                    walk_list(source, body, output)?;
                }
            }
        }
        CompoundCommand::Coprocess(group) => walk_command(source, &group.body, output, None)?,
        CompoundCommand::Arithmetic(group) => output.push(Record::Expansion(Word {
            raw: original(source, &group.loc)?.to_owned(),
            syntax: super::WordSyntax::Shell,
        })),
    }
    Ok(())
}

fn item_record(
    source: &Source<'_>,
    item: &CommandPrefixOrSuffixItem,
    argv: &mut Vec<Word>,
    redirects: &mut Vec<Redirect>,
    output: &mut Vec<Record>,
) -> Result<(), CheckError> {
    match item {
        CommandPrefixOrSuffixItem::Word(value) => argv.push(word(source, value)?),
        CommandPrefixOrSuffixItem::AssignmentWord(assignment, value) => {
            let value = word(source, value)?;
            if let AssignmentName::ArrayElementName(name, _) = &assignment.name {
                let left = name.len();
                let (lexical, error) = super::lexer::Lexed::parameter_fragment(
                    &value.raw,
                    super::lexer::Context::default(),
                );
                if error == Some(super::lexer::LexError::Nesting) {
                    return Err(CheckError {
                        kind: CheckErrorKind::ResourceLimit,
                    });
                }
                if let Some(right) = lexical.closing(left, b'[', b']') {
                    let index = value.raw.get(left + 1..right).ok_or(CheckError {
                        kind: CheckErrorKind::GuardFault,
                    })?;
                    output.push(Record::Expansion(Word {
                        raw: index.to_owned(),
                        syntax: super::WordSyntax::Arithmetic,
                    }));
                } else {
                    output.push(Record::UnsupportedSyntax);
                }
            }
            if !argv.is_empty() {
                argv.push(value);
                return Ok(());
            }
            if let Some((name, raw)) = value.raw.split_once('=') {
                let (name, target) = if let AssignmentValue::Scalar(target) = &assignment.value {
                    let mut name = assignment.name.to_string();
                    if assignment.append {
                        name.push('+');
                    }
                    (name, word(source, target)?)
                } else {
                    (
                        name.to_owned(),
                        Word {
                            raw: raw.to_owned(),
                            syntax: super::WordSyntax::Shell,
                        },
                    )
                };
                output.push(Record::Assignment(name, target));
            }
        }
        CommandPrefixOrSuffixItem::IoRedirect(value) => redirect(source, value, redirects, output)?,
        CommandPrefixOrSuffixItem::ProcessSubstitution(_, group) => {
            let mut records = Vec::new();
            walk_list(source, &group.list, &mut records)?;
            output.push(Record::Nested(records));
            argv.push(Word {
                raw: "__observed_stream__".into(),
                syntax: super::WordSyntax::Shell,
            });
        }
    }
    Ok(())
}

fn redirect_list(
    source: &Source<'_>,
    list: Option<&RedirectList>,
    output: &mut Vec<Record>,
) -> Result<(), CheckError> {
    if let Some(list) = list {
        let mut redirects = Vec::new();
        for value in &list.0 {
            redirect(source, value, &mut redirects, output)?;
        }
        output.push(Record::Command {
            argv: Vec::new(),
            redirects,
            pipeline: None,
        });
    }
    Ok(())
}

fn redirect(
    source: &Source<'_>,
    value: &IoRedirect,
    redirects: &mut Vec<Redirect>,
    output: &mut Vec<Record>,
) -> Result<(), CheckError> {
    match value {
        IoRedirect::File(
            _,
            kind,
            IoFileRedirectTarget::Filename(target) | IoFileRedirectTarget::Duplicate(target),
        ) => {
            redirects.push(Redirect {
                target: word(source, target)?,
                direction: if matches!(
                    kind,
                    IoFileRedirectKind::Read
                        | IoFileRedirectKind::ReadAndWrite
                        | IoFileRedirectKind::DuplicateInput
                ) {
                    crate::record::Direction::In
                } else {
                    crate::record::Direction::Out
                },
            });
        }
        IoRedirect::File(_, _, IoFileRedirectTarget::ProcessSubstitution(_, group)) => {
            let mut records = Vec::new();
            walk_list(source, &group.list, &mut records)?;
            output.push(Record::Nested(records));
        }
        IoRedirect::HereDocument(_, doc) => {
            let Some(span) = &doc.doc.loc else {
                output.push(Record::UnsupportedSyntax);
                return Ok(());
            };
            let span = source.range(span)?;
            let Some((range, quoted, strip_tabs)) = source.lexical.heredoc_body(span.start) else {
                output.push(Record::UnsupportedSyntax);
                return Ok(());
            };
            if range.end > span.end
                || *quoted == doc.requires_expansion
                || *strip_tabs != doc.remove_tabs
            {
                output.push(Record::UnsupportedSyntax);
            }
            let raw = source.text.get(range.clone()).ok_or(CheckError {
                kind: CheckErrorKind::GuardFault,
            })?;
            let raw = if *strip_tabs {
                raw.split_inclusive('\n')
                    .map(|line| line.trim_start_matches('\t'))
                    .collect::<String>()
            } else {
                raw.to_owned()
            };
            if raw != doc.doc.value {
                output.push(Record::UnsupportedSyntax);
            }
            let body = Word {
                raw,
                syntax: if !quoted {
                    super::WordSyntax::Heredoc
                } else {
                    super::WordSyntax::Literal
                },
            };
            if !quoted {
                output.push(Record::Expansion(body.clone()));
            }
            redirects.push(Redirect {
                target: body,
                direction: crate::record::Direction::Heredoc,
            });
        }
        IoRedirect::HereString(_, value) => {
            let body = word(source, value)?;
            output.push(Record::Expansion(body.clone()));
            redirects.push(Redirect {
                target: body,
                direction: crate::record::Direction::Herestring,
            });
        }
        IoRedirect::OutputAndError(value, _) => redirects.push(Redirect {
            target: word(source, value)?,
            direction: crate::record::Direction::Out,
        }),
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
                .any(|record| { matches!(record, Record::UnsupportedSyntax) })
        );
    }

    #[test]
    fn arithmetic_for_forwards_one_original_header_region() {
        let raw = "for ((i=0; i<1; i++)); do echo public; done";
        let parsed = records(raw, raw).unwrap().records.unwrap();
        let bodies: Vec<_> = parsed
            .iter()
            .filter_map(|record| match record {
                Record::Expansion(word) => Some(word.raw.as_str()),
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
                .any(|r| matches!(r, Record::UnsupportedSyntax))
        );
        assert!(
            output
                .iter()
                .any(|r| matches!(r, Record::Expansion(word) if word.raw == "public\n"))
        );
    }
}
