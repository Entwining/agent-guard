use super::{Operator, Parsed, RawRedirect as Redirect, RawWord as Word, Statement};
use std::ops::Range;

struct Source<'a> {
    text: &'a str,
    offsets: Vec<usize>,
    lexical: super::lexer::Lexed<'a>,
    expansions: Vec<(Range<usize>, super::RawExpansion)>,
}

impl Source<'_> {
    fn expansions(&self, range: &Range<usize>) -> Vec<super::RawExpansion> {
        self.expansions
            .iter()
            .filter(|(r, _)| range.start <= r.start && r.end <= range.end)
            .map(|(_, e)| e.clone())
            .collect()
    }
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
        expansions: parsed
            .loc
            .as_ref()
            .map(|span| source.range(span))
            .transpose()?
            .map_or_else(Vec::new, |range| source.expansions(&range)),
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
    let detection = super::divergence::detect_lexed(source, &[], &lexical)?;
    let mut expansions = detection
        .code_regions
        .into_iter()
        .map(|(r, c)| (r, super::RawExpansion::Code(c)))
        .chain(
            detection
                .evaluated_regions
                .into_iter()
                .map(|(r, n)| (r, super::RawExpansion::Variable(n))),
        )
        .collect::<Vec<_>>();
    expansions.sort_by_key(|(r, _)| r.start);
    let source = Source {
        text: source,
        lexical,
        expansions,
        offsets: parsed
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(parsed.len()))
            .collect(),
    };
    let tokens =
        brush_parser::uncached_tokenize_str(parsed, &brush_parser::TokenizerOptions::default());
    let mut spans: Vec<_> = std::iter::once(0..source.text.len()).collect();
    if let Ok(tokens) = &tokens {
        for token in tokens {
            spans.push(source.range(token.location())?);
        }
    }
    let Ok(mut tokens) = tokens else {
        return Ok(Parsed {
            records: None,
            spans,
        });
    };
    // Brush 0.4's optional final case pattern consumes `esac)` greedily.
    // A separator before the group closer is equivalent and retains source spans.
    for index in (1..tokens.len()).rev() {
        if tokens[index - 1].to_str() == "esac" && tokens[index].to_str() == ")" {
            let mut span = tokens[index].location().clone();
            span.end = span.start.clone();
            tokens.insert(index, brush_parser::Token::Operator("\n".into(), span));
        }
    }
    let Ok(program) = brush_parser::parse_tokens(&tokens, &brush_parser::ParserOptions::default())
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
    output: &mut Vec<Statement>,
) -> Result<(), CheckError> {
    for item in &list.0 {
        let mut statement = walk_pipeline(source, &item.0.first)?;
        for next in &item.0.additional {
            let (operator, pipeline) = match next {
                AndOr::And(pipeline) => (Operator::And, pipeline),
                AndOr::Or(pipeline) => (Operator::Or, pipeline),
            };
            statement = Statement::Binary(
                operator,
                Box::new(statement),
                Box::new(walk_pipeline(source, pipeline)?),
            );
        }
        output.push(if matches!(item.1, SeparatorOperator::Async) {
            Statement::Async(vec![statement])
        } else {
            statement
        });
    }
    Ok(())
}

fn walk_pipeline(source: &Source<'_>, pipeline: &Pipeline) -> Result<Statement, CheckError> {
    let id = if pipeline.seq.len() > 1 {
        pipeline
            .location()
            .map(|s| source.range(&s))
            .transpose()?
            .map(|s| s.start)
    } else {
        None
    };
    let mut commands = Vec::new();
    for command in &pipeline.seq {
        let mut records = Vec::new();
        walk_command(source, command, &mut records, id)?;
        commands.push(if records.len() == 1 {
            records.remove(0)
        } else {
            Statement::Group(records)
        });
    }
    let mut commands = commands.into_iter();
    let mut statement = commands.next().unwrap_or(Statement::Group(Vec::new()));
    for command in commands {
        statement = Statement::Binary(Operator::Pipe, Box::new(statement), Box::new(command));
    }
    Ok(statement)
}

fn walk_command(
    source: &Source<'_>,
    command: &Command,
    output: &mut Vec<Statement>,
    pipeline: Option<usize>,
) -> Result<(), CheckError> {
    match command {
        Command::Simple(simple) => {
            let mut assignments = Vec::new();
            let mut argv = Vec::new();
            let mut redirects = Vec::new();
            if let Some(prefix) = &simple.prefix {
                for item in &prefix.0 {
                    item_record(
                        source,
                        item,
                        &mut argv,
                        &mut redirects,
                        &mut assignments,
                        output,
                    )?;
                }
            }
            if let Some(name) = &simple.word_or_name {
                argv.push(word(source, name)?);
            }
            if let Some(suffix) = &simple.suffix {
                for item in &suffix.0 {
                    item_record(
                        source,
                        item,
                        &mut argv,
                        &mut redirects,
                        &mut assignments,
                        output,
                    )?;
                }
            }
            if !assignments.is_empty() || !argv.is_empty() || !redirects.is_empty() {
                output.push(Statement::Command {
                    assignments,
                    argv,
                    redirects,
                    pipeline,
                });
            }
        }
        Command::Compound(compound, redirects) => {
            redirect_list(source, redirects.as_ref(), output)?;
            walk_compound(source, compound, output)?;
        }
        Command::Function(function) => {
            let mut body = Vec::new();
            walk_compound(source, &function.body.0, &mut body)?;
            redirect_list(source, function.body.1.as_ref(), &mut body)?;
            output.push(Statement::Definition(function.fname.value.clone(), body));
        }
        Command::ExtendedTest(test, redirects) => {
            redirect_list(source, redirects.as_ref(), output)?;
            test_words(source, &test.expr, output)?;
        }
    }
    Ok(())
}

fn walk_compound(
    source: &Source<'_>,
    command: &CompoundCommand,
    output: &mut Vec<Statement>,
) -> Result<(), CheckError> {
    match command {
        CompoundCommand::BraceGroup(group) => {
            let mut body = Vec::new();
            walk_list(source, &group.list, &mut body)?;
            output.push(Statement::Group(body));
        }
        CompoundCommand::Subshell(group) => {
            let mut body = Vec::new();
            walk_list(source, &group.list, &mut body)?;
            output.push(Statement::Subshell(body));
        }
        CompoundCommand::ForClause(group) => {
            // brush 0.4 uses None for both an omitted list and an explicit empty list.
            let start = source.range(&group.loc)?.start;
            let end = source.range(&group.body.loc)?.start;
            let explicit_in = source.text[start..end]
                .match_indices("in")
                .any(|(offset, _)| {
                    let index = start + offset;
                    source.lexical.context(index).word_syntax()
                        && index > start
                        && super::lexer::shell_blank(source.text.as_bytes()[index - 1])
                        && source
                            .text
                            .as_bytes()
                            .get(index + 2)
                            .is_some_and(|b| super::lexer::shell_blank(*b) || *b == b';')
                });
            let mut header: Vec<Word> = group
                .values
                .iter()
                .flatten()
                .map(|v| word(source, v))
                .collect::<Result<_, _>>()?;
            if !explicit_in && group.values.is_none() {
                header.push(Word {
                    raw: "\"$@\"".into(),
                    syntax: super::WordSyntax::Shell,
                    expansions: Vec::new(),
                });
            }
            let mut body = Vec::new();
            walk_list(source, &group.body.list, &mut body)?;
            output.push(Statement::Loop {
                variable: Some(group.variable_name.clone()),
                header,
                body,
                empty: explicit_in && group.values.as_ref().is_none_or(Vec::is_empty),
            });
        }
        CompoundCommand::ArithmeticForClause(group) => {
            let range = source.range(&group.loc)?;
            let header = original(source, &group.loc)?;
            if let Some(left) = header.find("((").map(|i| i + range.start)
                && let Some(right) = source.lexical.closing(left, b'(', b')')
                && right < range.end
            {
                output.push(Statement::Expansion(Word {
                    raw: source.text[left + 2..right - 1].to_owned(),
                    syntax: super::WordSyntax::Arithmetic,
                    expansions: source.expansions(&(left + 2..right - 1)),
                }));
            } else {
                output.push(Statement::UnsupportedSyntax);
            }
            let mut body = Vec::new();
            walk_list(source, &group.body.list, &mut body)?;
            output.push(Statement::Loop {
                variable: None,
                header: Vec::new(),
                body,
                empty: false,
            });
        }
        CompoundCommand::IfClause(group) => {
            let mut condition = Vec::new();
            walk_list(source, &group.condition, &mut condition)?;
            let mut then = Vec::new();
            walk_list(source, &group.then, &mut then)?;
            let mut otherwise = Vec::new();
            if let Some(elses) = &group.elses {
                for branch in elses.iter().rev() {
                    let mut body = Vec::new();
                    walk_list(source, &branch.body, &mut body)?;
                    if let Some(test) = &branch.condition {
                        let mut condition = Vec::new();
                        walk_list(source, test, &mut condition)?;
                        otherwise = vec![Statement::Conditional {
                            condition,
                            then: body,
                            otherwise,
                        }];
                    } else {
                        otherwise = body;
                    }
                }
            }
            output.push(Statement::Conditional {
                condition,
                then,
                otherwise,
            });
        }
        CompoundCommand::WhileClause(group) | CompoundCommand::UntilClause(group) => {
            let mut body = Vec::new();
            walk_list(source, &group.0, &mut body)?;
            walk_list(source, &group.1.list, &mut body)?;
            output.push(Statement::Loop {
                variable: None,
                header: Vec::new(),
                body,
                empty: false,
            });
        }
        CompoundCommand::CaseClause(group) => {
            let mut words = vec![word(source, &group.value)?];
            let mut branches = Vec::new();
            let mut exhaustive = false;
            for case in &group.cases {
                for pattern in &case.patterns {
                    let w = word(source, pattern)?;
                    exhaustive |= w.raw == "*";
                    words.push(w);
                }
                let mut body = Vec::new();
                if let Some(commands) = &case.cmd {
                    walk_list(source, commands, &mut body)?;
                }
                branches.push(body);
            }
            output.push(Statement::Case {
                words,
                branches,
                exhaustive,
            });
        }
        CompoundCommand::Coprocess(group) => walk_command(source, &group.body, output, None)?,
        CompoundCommand::Arithmetic(group) => output.push(Statement::Expansion(Word {
            raw: group.expr.value.clone(),
            syntax: super::WordSyntax::Arithmetic,
            expansions: source.expansions(&source.range(&group.loc)?),
        })),
    }
    Ok(())
}

fn test_words(
    source: &Source<'_>,
    expr: &ExtendedTestExpr,
    output: &mut Vec<Statement>,
) -> Result<(), CheckError> {
    match expr {
        ExtendedTestExpr::And(a, b) | ExtendedTestExpr::Or(a, b) => {
            test_words(source, a, output)?;
            test_words(source, b, output)?;
        }
        ExtendedTestExpr::Not(e) | ExtendedTestExpr::Parenthesized(e) => {
            test_words(source, e, output)?
        }
        ExtendedTestExpr::UnaryTest(_, w) => output.push(Statement::Use(word(source, w)?)),
        ExtendedTestExpr::BinaryTest(predicate, a, b) => {
            for w in [a, b] {
                let mut value = word(source, w)?;
                if matches!(predicate, BinaryPredicate::ArithmeticEqualTo) {
                    value.syntax = super::WordSyntax::Arithmetic;
                }
                output.push(Statement::Use(value));
            }
        }
    }
    Ok(())
}

fn item_record(
    source: &Source<'_>,
    item: &CommandPrefixOrSuffixItem,
    argv: &mut Vec<Word>,
    redirects: &mut Vec<Redirect>,
    assignments: &mut Vec<(String, Word)>,
    output: &mut Vec<Statement>,
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
                    output.push(Statement::Expansion(Word {
                        raw: index.to_owned(),
                        syntax: super::WordSyntax::Arithmetic,
                        expansions: value.expansions.clone(),
                    }));
                } else {
                    output.push(Statement::UnsupportedSyntax);
                }
            }
            if !argv.is_empty() {
                argv.push(value);
                return Ok(());
            }
            if let AssignmentValue::Array(elements) = &assignment.value {
                let mut values = Vec::new();
                for (index, value) in elements {
                    if let Some(index) = index {
                        let mut index = word(source, index)?;
                        index.syntax = super::WordSyntax::Arithmetic;
                        output.push(Statement::Expansion(index));
                    }
                    values.push(word(source, value)?);
                }
                output.push(Statement::ArrayAssignment {
                    name: assignment.name.to_string(),
                    values,
                    append: assignment.append,
                });
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
                            expansions: value.expansions.clone(),
                        },
                    )
                };
                assignments.push((name, target));
            }
        }
        CommandPrefixOrSuffixItem::IoRedirect(value) => redirect(source, value, redirects, output)?,
        CommandPrefixOrSuffixItem::ProcessSubstitution(_, group) => {
            let mut records = Vec::new();
            walk_list(source, &group.list, &mut records)?;
            output.push(Statement::Substitution(records));
            argv.push(Word {
                raw: "__observed_stream__".into(),
                syntax: super::WordSyntax::Shell,
                expansions: Vec::new(),
            });
        }
    }
    Ok(())
}

fn redirect_list(
    source: &Source<'_>,
    list: Option<&RedirectList>,
    output: &mut Vec<Statement>,
) -> Result<(), CheckError> {
    if let Some(list) = list {
        let mut redirects = Vec::new();
        for value in &list.0 {
            redirect(source, value, &mut redirects, output)?;
        }
        output.push(Statement::Command {
            assignments: Vec::new(),
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
    output: &mut Vec<Statement>,
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
            output.push(Statement::Substitution(records));
        }
        IoRedirect::HereDocument(_, doc) => {
            let Some(span) = &doc.doc.loc else {
                output.push(Statement::UnsupportedSyntax);
                return Ok(());
            };
            let span = source.range(span)?;
            let Some((range, quoted, strip_tabs)) = source.lexical.heredoc_body(span.start) else {
                output.push(Statement::UnsupportedSyntax);
                return Ok(());
            };
            if range.end > span.end
                || *quoted == doc.requires_expansion
                || *strip_tabs != doc.remove_tabs
            {
                output.push(Statement::UnsupportedSyntax);
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
                output.push(Statement::UnsupportedSyntax);
            }
            let body = Word {
                raw,
                expansions: source.expansions(range),
                syntax: if !quoted {
                    super::WordSyntax::Heredoc
                } else {
                    super::WordSyntax::Literal
                },
            };
            if !quoted {
                output.push(Statement::Expansion(body.clone()));
            }
            redirects.push(Redirect {
                target: body,
                direction: crate::record::Direction::Heredoc,
            });
        }
        IoRedirect::HereString(_, value) => {
            let body = word(source, value)?;
            output.push(Statement::Expansion(body.clone()));
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
}
