use super::{Parsed, Record, Redirect, Word};
use std::ops::Range;

struct Source<'a> {
    text: &'a str,
    offsets: Vec<usize>,
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
    })
}
type WordAst = brush_parser::ast::Word;

fn original<'a>(source: &'a Source<'_>, span: &SourceSpan) -> Result<&'a str, CheckError> {
    source.text.get(source.range(span)?).ok_or(CheckError {
        kind: CheckErrorKind::GuardFault,
    })
}

pub(super) fn records(source: &str, parsed: &str) -> Result<Parsed, CheckError> {
    let source = Source {
        text: source,
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
            for expression in [&group.initializer, &group.condition, &group.updater]
                .into_iter()
                .flatten()
            {
                output.push(Record::Expansion(Word {
                    raw: expression.to_string(),
                }));
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
        CommandPrefixOrSuffixItem::AssignmentWord(_, value) => {
            let value = word(source, value)?;
            if !argv.is_empty() {
                argv.push(value);
                return Ok(());
            }
            if let Some((name, raw)) = value.raw.split_once('=') {
                output.push(Record::Assignment(
                    name.to_owned(),
                    Word {
                        raw: raw.to_owned(),
                    },
                ));
            }
        }
        CommandPrefixOrSuffixItem::IoRedirect(value) => redirect(source, value, redirects, output)?,
        CommandPrefixOrSuffixItem::ProcessSubstitution(_, group) => {
            let mut records = Vec::new();
            walk_list(source, &group.list, &mut records)?;
            output.push(Record::Nested(records));
            argv.push(Word {
                raw: "__observed_stream__".into(),
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
                write: !matches!(
                    kind,
                    IoFileRedirectKind::Read
                        | IoFileRedirectKind::ReadAndWrite
                        | IoFileRedirectKind::DuplicateInput
                ),
            });
        }
        IoRedirect::File(_, _, IoFileRedirectTarget::ProcessSubstitution(_, group)) => {
            let mut records = Vec::new();
            walk_list(source, &group.list, &mut records)?;
            output.push(Record::Nested(records));
        }
        IoRedirect::HereDocument(_, doc) if doc.requires_expansion => {
            output.push(Record::Expansion(Word {
                raw: doc.doc.value.clone(),
            }))
        }
        IoRedirect::HereString(_, value) => output.push(Record::Expansion(word(source, value)?)),
        IoRedirect::OutputAndError(value, _) => redirects.push(Redirect {
            target: word(source, value)?,
            write: true,
        }),
        _ => {}
    }
    Ok(())
}
