use super::Expanded;
use crate::{CheckError, CheckErrorKind, record::Word};
use brush_parser::{
    ParserOptions,
    word::{self, Parameter, ParameterExpr, TildeExpr, WordPiece, WordPieceWithSource},
};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

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
            arithmetic: Vec::new(),
            references: Vec::new(),
            parameters: Vec::new(),
            unsupported: false,
        });
    }
    let heredoc = matches!(syntax, super::WordSyntax::Heredoc);
    let arithmetic = matches!(syntax, super::WordSyntax::Arithmetic);
    let options = ParserOptions::default();
    let (input, braces) = if heredoc || arithmetic {
        (raw.to_owned(), false)
    } else {
        brace_text(raw)?
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
        arithmetic: Vec::new(),
        references: Vec::new(),
        parameters: Vec::new(),
        unsupported: false,
    };
    out.word.raw = raw.to_owned();
    out.word.globs = braces;
    let mut splitting = false;
    let (lexical, error) = super::lexer::Lexed::parameter_fragment(
        &input,
        super::lexer::Context {
            heredoc: heredoc.then_some(false),
            arithmetic_depth: usize::from(arithmetic),
            ..super::lexer::Context::default()
        },
    );
    match error {
        Some(super::lexer::LexError::Nesting) => {
            return Err(CheckError {
                kind: CheckErrorKind::ResourceLimit,
            });
        }
        Some(super::lexer::LexError::Unterminated { .. }) => out.unsupported = true,
        None => {}
    }
    out.parameters = parameter_regions(&input, &lexical);
    fill(
        &input,
        &pieces,
        &lexical,
        variables,
        host,
        &mut out,
        &mut splitting,
    )?;
    out.unsupported |= out.parameters.iter().any(|region| !region.supported);
    if input != raw && !out.parameters.is_empty() {
        let original = super::lexer::Lexed::scan(raw).map_err(|_| CheckError {
            kind: CheckErrorKind::GuardFault,
        })?;
        let original_regions = parameter_regions(raw, &original);
        if original_regions.len() != out.parameters.len() {
            return Err(CheckError {
                kind: CheckErrorKind::GuardFault,
            });
        }
        for (region, original) in out.parameters.iter_mut().zip(original_regions) {
            region.range = original.range;
        }
    }
    if arithmetic {
        out.arithmetic.push(raw.to_owned());
    }
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

fn brace_text(raw: &str) -> Result<(String, bool), CheckError> {
    let lexical = match super::lexer::Lexed::scan(raw) {
        Ok(lexical) => lexical,
        Err(super::lexer::LexError::Unterminated { .. }) => return Ok((raw.to_owned(), false)),
        Err(super::lexer::LexError::Nesting) => {
            return Err(CheckError {
                kind: CheckErrorKind::ResourceLimit,
            });
        }
    };
    let mut text = String::new();
    let mut expands = false;
    let mut cursor = 0;
    while cursor < raw.len() {
        let ch = raw[cursor..].chars().next().unwrap_or_default();
        if lexical.context(cursor).word_syntax()
            && ch == '{'
            && let Some((right, list)) = brace_group(raw, cursor, &lexical)
        {
            let group = &raw[cursor..=right];
            if raw[..cursor].ends_with('$') && lexical.context(cursor - 1).active() {
                text.push_str(group);
            } else {
                let body = &raw[cursor + 1..right];
                let (inner, nested) = brace_text(body)?;
                if let Some(sequence) = brace_sequence(body) {
                    text.push_str(&sequence);
                    expands = true;
                } else {
                    text.push('{');
                    text.push_str(&inner);
                    text.push('}');
                    expands |= nested || list;
                }
            }
            cursor = right + 1;
            continue;
        }
        text.push(ch);
        cursor += ch.len_utf8();
    }
    Ok((text, expands))
}

fn brace_group(raw: &str, left: usize, lexical: &super::lexer::Lexed<'_>) -> Option<(usize, bool)> {
    let mut depth = 0;
    let mut list = false;
    let mut cursor = left;
    while cursor < raw.len() {
        let ch = raw[cursor..].chars().next().unwrap_or_default();
        if lexical.context(cursor).word_syntax() && ch == '{' {
            depth += 1;
        }
        if lexical.context(cursor).word_syntax() && ch == '}' {
            depth -= 1;
            if depth == 0 {
                return Some((cursor, list));
            }
        }
        if lexical.context(cursor).word_syntax() && ch == ',' && depth == 1 {
            list = true;
        }
        cursor += ch.len_utf8();
    }
    None
}

fn brace_sequence(body: &str) -> Option<String> {
    let parts: Vec<_> = body.split("..").collect();
    if !(2..=3).contains(&parts.len()) || parts.get(2).is_some_and(|s| s.parse::<i128>().is_err()) {
        return None;
    }
    if let (Ok(start), Ok(end)) = (parts[0].parse::<i128>(), parts[1].parse::<i128>()) {
        if parts[..2].iter().any(|part| part.starts_with(['+', '-'])) {
            // Signed numeric reach cannot consume an empty string or a letter.
            // A leading plus may also stay literal in zsh (D1/D26).
            let reach = if start == end {
                start.to_string()
            } else {
                "[-0-9]*".into()
            };
            return Some(format!("{{{reach},{{{body}}}}}"));
        }
        return Some("*".into());
    }
    if parts[..2]
        .iter()
        .all(|part| part.len() == 1 && part.as_bytes()[0].is_ascii_alphanumeric())
    {
        // Zsh accepts mixed letter/digit endpoints; Bash may leave them literal.
        return Some("*".into());
    }
    None
}

fn parameter_regions(raw: &str, lexical: &super::lexer::Lexed<'_>) -> Vec<super::ParameterRegion> {
    raw.match_indices("${")
        .filter_map(|(start, _)| {
            let context = lexical.context(start);
            (context.active() && context.command_depth == 0 && context.backtick_depth == 0).then(
                || super::ParameterRegion {
                    range: start
                        ..lexical
                            .closing(start + 1, b'{', b'}')
                            .map_or(raw.len(), |end| end + 1),
                    supported: false,
                },
            )
        })
        .collect()
}

fn fragment(
    raw: &str,
    context: super::lexer::Context,
    variables: &BTreeMap<String, String>,
    host: crate::record::HostFacts<'_>,
) -> Result<Expanded, CheckError> {
    let mut out = Expanded {
        word: Word::literal(String::new()),
        split: Vec::new(),
        nested: Vec::new(),
        arithmetic: Vec::new(),
        references: Vec::new(),
        parameters: Vec::new(),
        unsupported: false,
    };
    let (lexical, error) = super::lexer::Lexed::parameter_fragment(raw, context);
    match error {
        Some(super::lexer::LexError::Nesting) => {
            return Err(CheckError {
                kind: CheckErrorKind::ResourceLimit,
            });
        }
        Some(super::lexer::LexError::Unterminated { .. }) => out.unsupported = true,
        None => {}
    }
    out.parameters = parameter_regions(raw, &lexical);
    let pieces = if matches!(
        context.quote,
        super::lexer::Quote::Double | super::lexer::Quote::Gettext
    ) {
        word::parse_heredoc(raw, &ParserOptions::default())
    } else {
        word::parse(raw, &ParserOptions::default())
    };
    match pieces {
        Ok(pieces) => fill(
            raw, &pieces, &lexical, variables, host, &mut out, &mut false,
        )?,
        Err(_) => out.unsupported = true,
    }
    out.unsupported |= out.parameters.iter().any(|r| !r.supported);
    Ok(out)
}

fn merge_fragment(out: &mut Expanded, inner: Expanded, offset: usize) {
    out.unsupported |= inner.unsupported;
    for region in inner.parameters {
        if let Some(parent) = out
            .parameters
            .iter_mut()
            .find(|parent| parent.range.start == offset + region.range.start)
        {
            parent.supported = region.supported;
        }
    }
    out.word.vars.extend(inner.word.vars);
    out.nested.extend(inner.nested);
    out.arithmetic.extend(inner.arithmetic);
    out.references.extend(inner.references);
}

fn fill(
    raw: &str,
    pieces: &[WordPieceWithSource],
    lexical: &super::lexer::Lexed<'_>,
    variables: &BTreeMap<String, String>,
    host: crate::record::HostFacts<'_>,
    out: &mut Expanded,
    splitting: &mut bool,
) -> Result<(), CheckError> {
    let mut covered = 0;
    for piece in pieces {
        let context = lexical.context(piece.start_index);
        let quoted = !context.unquoted() || context.heredoc.is_some();
        // Brush word offsets are UTF-8 byte offsets, unlike its program SourceSpan.
        let spelling = raw
            .get(piece.start_index..piece.end_index)
            .ok_or(CheckError {
                kind: CheckErrorKind::GuardFault,
            })?;
        if piece.end_index <= covered {
            continue;
        }
        if piece.start_index < covered {
            if matches!(piece.piece, WordPiece::Text(_)) {
                out.word
                    .text
                    .push_str(&raw[covered..piece.end_index].replace("\\\n", ""));
            } else {
                out.unsupported = true;
            }
            continue;
        }
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
                fill(raw, inner, lexical, variables, host, out, splitting)?
            }
            WordPiece::EscapeSequence(text) => {
                let text = text.strip_prefix('\\').unwrap_or(text);
                if text != "\n" {
                    out.word.text.push_str(text);
                }
            }
            WordPiece::TildeExpansion(tilde) => {
                let value = match tilde {
                    TildeExpr::Home => {
                        out.word.vars.push("HOME".into());
                        variables
                            .get("HOME")
                            .map(String::as_str)
                            .or(Some(host.home))
                    }
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
                if let Some(Parameter::NamedWithIndex { index, .. }) = parameter(expr) {
                    out.references.push(index.clone());
                }
                if let ParameterExpr::Substring { offset, length, .. } = expr {
                    out.references.push(offset.value.clone());
                    if let Some(length) = length {
                        out.references.push(length.value.clone());
                    }
                }
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
                if spelling.starts_with("${")
                    && lexical.closing(piece.start_index + 1, b'{', b'}')
                        == Some(piece.end_index - 1)
                    && let Some(region) = out
                        .parameters
                        .iter_mut()
                        .find(|r| r.range.start == piece.start_index)
                {
                    region.supported = true;
                }
                for body in parameter_fragments(expr) {
                    let context = super::lexer::Context {
                        quote: context.quote,
                        parameter_depth: 1,
                        ..super::lexer::Context::default()
                    };
                    let inner = fragment(body, context, variables, host)?;
                    let offset = spelling.find(body).ok_or(CheckError {
                        kind: CheckErrorKind::GuardFault,
                    })?;
                    merge_fragment(out, inner, piece.start_index + offset);
                }
                if let Some(value) = plain.and_then(|name| {
                    variables
                        .get(name)
                        .map(String::as_str)
                        .or_else(|| (name == "HOME").then_some(host.home))
                }) {
                    out.word.pwd |= plain.is_some_and(|name| name == "PWD");
                    out.word.text.push_str(value);
                    *splitting |= !quoted;
                    // D1 retains Bash pathname expansion even when Zsh leaves the binding literal.
                    out.word.globs |= !quoted && value.contains(['*', '?', '[']);
                } else {
                    out.word.text.push_str(spelling);
                    out.word.expands = true;
                }
            }
            WordPiece::CommandSubstitution(_) | WordPiece::BackquotedCommandSubstitution(_) => {
                let Some(body) = lexical.substitution_body(piece.start_index) else {
                    out.unsupported = true;
                    out.word.expands = true;
                    continue;
                };
                let right = body.end;
                let code = &raw[body];
                out.nested.push(code.to_owned());
                if right + 1 != piece.end_index {
                    out.unsupported |= right + 1 < piece.end_index;
                    covered = right + 1;
                    out.word.expands = true;
                }
                if prints_pwd(code) && right + 1 == piece.end_index {
                    out.word.pwd = true;
                    if let Some(cwd) = variables.get("PWD") {
                        out.word.text.push_str(cwd);
                    }
                } else {
                    out.word.text.push_str(&raw[piece.start_index..right + 1]);
                    out.word.expands = true;
                }
            }
            WordPiece::ArithmeticExpression(expr) => {
                out.arithmetic.push(expr.value.clone());
                let (open, close) = if spelling.starts_with("$[") {
                    (b'[', b']')
                } else {
                    (b'(', b')')
                };
                out.unsupported |= lexical.closing(piece.start_index + 1, open, close)
                    != Some(piece.end_index - 1);
                let inner = fragment(
                    &expr.value,
                    super::lexer::Context {
                        arithmetic_depth: 1,
                        ..super::lexer::Context::default()
                    },
                    variables,
                    host,
                )?;
                let offset = spelling.find(&expr.value).ok_or(CheckError {
                    kind: CheckErrorKind::GuardFault,
                })?;
                merge_fragment(out, inner, piece.start_index + offset);
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

pub(super) fn ansi(text: &str) -> String {
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
