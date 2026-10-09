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
    Ok(match commands.len() {
        0 => Statement::Group(Vec::new()),
        1 => commands.remove(0),
        _ => Statement::Pipeline(commands),
    })
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
            let mut body = Vec::new();
            walk_compound(source, compound, &mut body)?;
            if redirects.is_some() {
                let mut prefix = Vec::new();
                redirect_list(source, redirects.as_ref(), &mut prefix)?;
                if let Some(Statement::Command { redirects, .. }) = prefix.pop() {
                    output.extend(prefix);
                    output.push(Statement::Redirected(redirects, body));
                }
            } else {
                output.extend(body);
            }
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

mod compound;
mod items;
mod redirects;
use compound::walk_compound;
use items::item_record;
use redirects::{redirect, redirect_list};
#[cfg(test)]
mod tests;
