use super::Expanded;
use crate::{CheckError, CheckErrorKind, record::Word};
use brush_parser::{
    ParserOptions,
    word::{
        self, BraceExpressionMember, BraceExpressionOrText, Parameter, ParameterExpr, TildeExpr,
        WordPiece, WordPieceWithSource,
    },
};
use std::collections::BTreeMap;

pub(super) fn expand(
    raw: &str,
    syntax: &super::WordSyntax,
    variables: &BTreeMap<String, String>,
    host: crate::record::HostFacts<'_>,
) -> Result<Expanded, CheckError> {
    if matches!(syntax, super::WordSyntax::Literal) {
        let word = Word::literal(raw.to_owned());
        return Ok(Expanded {
            split: vec![word.clone()],
            word,
            nested: Vec::new(),
        });
    }
    let heredoc = matches!(syntax, super::WordSyntax::Heredoc);
    let options = ParserOptions::default();
    let (input, braces) = if heredoc {
        (raw.to_owned(), false)
    } else {
        match word::parse_brace_expansions(raw, &options).map_err(|_| CheckError {
            kind: CheckErrorKind::GuardFault,
        })? {
            Some(parts) => brace_text(&parts),
            None => (raw.to_owned(), false),
        }
    };
    let pieces = if heredoc {
        word::parse_heredoc(&input, &options)
    } else {
        word::parse(&input, &options)
    }
    .map_err(|_| CheckError {
        kind: CheckErrorKind::GuardFault,
    })?;
    let mut out = Expanded {
        word: Word::literal(String::new()),
        split: Vec::new(),
        nested: Vec::new(),
    };
    out.word.raw = raw.to_owned();
    out.word.globs = braces;
    let mut splitting = false;
    fill(
        &input,
        &pieces,
        heredoc,
        variables,
        host,
        &mut out,
        &mut splitting,
    )?;
    out.word.value = out.word.text.clone();
    out.split = if splitting {
        out.word
            .text
            .split_whitespace()
            .map(|text| {
                let mut word = out.word.clone();
                word.text = text.to_owned();
                word.value = word.text.clone();
                word
            })
            .collect()
    } else {
        vec![out.word.clone()]
    };
    Ok(out)
}

fn brace_text(parts: &[BraceExpressionOrText]) -> (String, bool) {
    let mut text = String::new();
    let mut expands = false;
    for part in parts {
        match part {
            BraceExpressionOrText::Text(value) => text.push_str(value),
            BraceExpressionOrText::Expr(members) => {
                expands = true;
                if members.len() == 1
                    && matches!(
                        members[0],
                        BraceExpressionMember::NumberSequence { .. }
                            | BraceExpressionMember::CharSequence { .. }
                    )
                {
                    // Go models a sequence's reach as a glob rather than enumerating it.
                    text.push('*');
                } else {
                    text.push('{');
                    for (index, member) in members.iter().enumerate() {
                        if index > 0 {
                            text.push(',');
                        }
                        match member {
                            BraceExpressionMember::Child(children) => {
                                text.push_str(&brace_text(children).0)
                            }
                            _ => text.push('*'),
                        }
                    }
                    text.push('}');
                }
            }
        }
    }
    (text, expands)
}

fn fill(
    raw: &str,
    pieces: &[WordPieceWithSource],
    quoted: bool,
    variables: &BTreeMap<String, String>,
    host: crate::record::HostFacts<'_>,
    out: &mut Expanded,
    splitting: &mut bool,
) -> Result<(), CheckError> {
    for piece in pieces {
        // Brush word offsets are UTF-8 byte offsets, unlike its program SourceSpan.
        let spelling = raw
            .get(piece.start_index..piece.end_index)
            .ok_or(CheckError {
                kind: CheckErrorKind::GuardFault,
            })?;
        match &piece.piece {
            WordPiece::Text(text) => {
                out.word.text.push_str(&text.replace("\\\n", ""));
                if !quoted {
                    out.word.globs |= text.contains(['*', '?', '[', '(']);
                }
            }
            WordPiece::SingleQuotedText(text) => out.word.text.push_str(text),
            WordPiece::AnsiCQuotedText(text) => out.word.text.push_str(&ansi(text)),
            WordPiece::DoubleQuotedSequence(inner)
            | WordPiece::GettextDoubleQuotedSequence(inner) => {
                fill(raw, inner, true, variables, host, out, splitting)?
            }
            WordPiece::EscapeSequence(text) => {
                let text = text.strip_prefix('\\').unwrap_or(text);
                if text != "\n" {
                    out.word.text.push_str(text);
                }
            }
            WordPiece::TildeExpansion(tilde) => {
                let value = match tilde {
                    TildeExpr::Home => Some(host.home),
                    TildeExpr::UserHome(user) if user == host.user.unwrap_or("unknown") => {
                        Some(host.home)
                    }
                    TildeExpr::WorkingDir => {
                        out.word.pwd = true;
                        variables.get("PWD").map(String::as_str)
                    }
                    _ => None,
                };
                out.word.text.push_str(value.unwrap_or(spelling));
            }
            WordPiece::ParameterExpansion(expr) => {
                let plain = if let ParameterExpr::Parameter {
                    parameter: Parameter::Named(name),
                    indirect: false,
                } = expr
                {
                    Some(name)
                } else {
                    None
                };
                if let Some(name) = parameter_name(expr) {
                    out.word.vars.push(name);
                }
                for fragment in parameter_fragments(expr) {
                    let inner = word::parse(fragment, &ParserOptions::default()).map_err(|_| {
                        CheckError {
                            kind: CheckErrorKind::GuardFault,
                        }
                    })?;
                    let mut expansion = Expanded {
                        word: Word::literal(String::new()),
                        split: Vec::new(),
                        nested: Vec::new(),
                    };
                    fill(
                        fragment,
                        &inner,
                        true,
                        variables,
                        host,
                        &mut expansion,
                        &mut false,
                    )?;
                    out.word.vars.extend(expansion.word.vars);
                    out.nested.extend(expansion.nested);
                }
                if let Some(value) = plain.and_then(|name| {
                    if name == "HOME" {
                        Some(host.home)
                    } else {
                        variables.get(name).map(String::as_str)
                    }
                }) {
                    out.word.pwd |= plain.is_some_and(|name| name == "PWD");
                    out.word.text.push_str(value);
                    *splitting |= !quoted;
                } else {
                    out.word.text.push_str(spelling);
                    out.word.expands = true;
                }
            }
            WordPiece::CommandSubstitution(code)
            | WordPiece::BackquotedCommandSubstitution(code) => {
                out.nested.push(code.clone());
                if prints_pwd(code) {
                    out.word.pwd = true;
                    if let Some(cwd) = variables.get("PWD") {
                        out.word.text.push_str(cwd);
                    }
                } else {
                    out.word.text.push_str(spelling);
                    out.word.expands = true;
                }
            }
            WordPiece::ArithmeticExpression(expr) => {
                let pieces = word::parse(&expr.value, &ParserOptions::default()).map_err(|_| {
                    CheckError {
                        kind: CheckErrorKind::GuardFault,
                    }
                })?;
                let mut inner = Expanded {
                    word: Word::literal(String::new()),
                    split: Vec::new(),
                    nested: Vec::new(),
                };
                fill(
                    &expr.value,
                    &pieces,
                    true,
                    variables,
                    host,
                    &mut inner,
                    &mut false,
                )?;
                out.word.vars.extend(inner.word.vars);
                out.nested.extend(inner.nested);
                out.word.text.push_str(spelling);
                out.word.expands = true;
            }
        }
    }
    Ok(())
}

fn parameter(expr: &ParameterExpr) -> Option<&Parameter> {
    use ParameterExpr::*;
    let parameter = match expr {
        Parameter { parameter, .. }
        | UseDefaultValues { parameter, .. }
        | AssignDefaultValues { parameter, .. }
        | IndicateErrorIfNullOrUnset { parameter, .. }
        | UseAlternativeValue { parameter, .. }
        | ParameterLength { parameter, .. }
        | RemoveSmallestSuffixPattern { parameter, .. }
        | RemoveLargestSuffixPattern { parameter, .. }
        | RemoveSmallestPrefixPattern { parameter, .. }
        | RemoveLargestPrefixPattern { parameter, .. }
        | Substring { parameter, .. }
        | Transform { parameter, .. }
        | UppercaseFirstChar { parameter, .. }
        | UppercasePattern { parameter, .. }
        | LowercaseFirstChar { parameter, .. }
        | LowercasePattern { parameter, .. }
        | ReplaceSubstring { parameter, .. } => parameter,
        VariableNames { .. } | MemberKeys { .. } => return None,
    };
    Some(parameter)
}

fn parameter_name(expr: &ParameterExpr) -> Option<String> {
    match parameter(expr) {
        Some(
            Parameter::Named(name)
            | Parameter::NamedWithIndex { name, .. }
            | Parameter::NamedWithAllIndices { name, .. },
        ) => Some(name.clone()),
        Some(Parameter::Positional(index)) => Some(index.to_string()),
        Some(Parameter::Special(value)) => Some(value.to_string()),
        None => match expr {
            ParameterExpr::VariableNames { prefix, .. } => Some(prefix.clone()),
            ParameterExpr::MemberKeys { variable_name, .. } => Some(variable_name.clone()),
            _ => None,
        },
    }
}

fn parameter_fragments(expr: &ParameterExpr) -> Vec<&str> {
    use ParameterExpr::*;
    let mut fragments = Vec::new();
    if let Some(word::Parameter::NamedWithIndex { index, .. }) = parameter(expr) {
        fragments.push(index.as_str());
    }
    let optional = match expr {
        UseDefaultValues { default_value, .. } | AssignDefaultValues { default_value, .. } => {
            default_value.as_deref()
        }
        UseAlternativeValue {
            alternative_value, ..
        } => alternative_value.as_deref(),
        IndicateErrorIfNullOrUnset { error_message, .. } => error_message.as_deref(),
        RemoveSmallestSuffixPattern { pattern, .. }
        | RemoveLargestSuffixPattern { pattern, .. }
        | RemoveSmallestPrefixPattern { pattern, .. }
        | RemoveLargestPrefixPattern { pattern, .. }
        | UppercaseFirstChar { pattern, .. }
        | UppercasePattern { pattern, .. }
        | LowercaseFirstChar { pattern, .. }
        | LowercasePattern { pattern, .. } => pattern.as_deref(),
        ReplaceSubstring {
            pattern,
            replacement,
            ..
        } => {
            fragments.push(pattern);
            replacement.as_deref()
        }
        Substring { offset, length, .. } => {
            fragments.push(&offset.value);
            length.as_ref().map(|value| value.value.as_str())
        }
        _ => None,
    };
    fragments.extend(optional);
    fragments
}

fn prints_pwd(code: &str) -> bool {
    let words: Vec<_> = code.split_whitespace().collect();
    words == ["pwd"] || words == ["pwd", "-L"] || words == ["pwd", "-P"]
}

fn ansi(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(ch) = chars.next() else {
            out.push('\\');
            break;
        };
        let (radix, limit) = match ch {
            'x' => (16, 2),
            'u' => (16, 4),
            '0'..='7' => (8, 3),
            _ => (0, 0),
        };
        if radix != 0 {
            let mut digits = String::new();
            if radix == 8 {
                digits.push(ch);
            }
            while digits.len() < limit && chars.peek().is_some_and(|ch| ch.is_digit(radix)) {
                if let Some(ch) = chars.next() {
                    digits.push(ch);
                }
            }
            // Go accepts one or two hex digits, exactly four Unicode digits, or up to three octal digits.
            if !digits.is_empty() && (ch != 'u' || digits.len() == 4) {
                out.push(
                    u32::from_str_radix(&digits, radix)
                        .ok()
                        .and_then(char::from_u32)
                        .unwrap_or('\u{fffd}'),
                );
                continue;
            }
            out.push(ch);
            out.push_str(&digits);
            continue;
        }
        out.push(match ch {
            'a' => '\u{7}',
            'b' => '\u{8}',
            'e' => '\u{1b}',
            'f' => '\u{c}',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'v' => '\u{b}',
            other => other,
        });
    }
    out
}
