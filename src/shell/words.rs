use super::Expanded;
use crate::{CheckError, CheckErrorKind, record::Word};
use brush_parser::{
    ParserOptions,
    word::{self, Parameter, ParameterExpr, TildeExpr, WordPiece, WordPieceWithSource},
};
use std::collections::BTreeMap;

mod modifiers;
mod parameters;

#[cfg(test)]
mod tests;

pub(super) struct PositionalList {
    pub offset: String,
    pub length: Option<String>,
    pub concatenate: bool,
    pub quoted: bool,
}

pub(super) fn positional_list(raw: &str) -> Option<PositionalList> {
    let pieces = word::parse(raw, &ParserOptions::default()).ok()?;
    let [piece] = pieces.as_slice() else {
        return None;
    };
    let (piece, quoted) = match &piece.piece {
        WordPiece::DoubleQuotedSequence(inner) => {
            let [piece] = inner.as_slice() else {
                return None;
            };
            (&piece.piece, true)
        }
        piece => (piece, false),
    };
    let WordPiece::ParameterExpansion(expr) = piece else {
        return None;
    };
    let Parameter::Special(word::SpecialParameter::AllPositionalParameters { concatenate }) =
        parameter(expr)?
    else {
        return None;
    };
    let (offset, length) = match expr {
        ParameterExpr::Parameter {
            indirect: false, ..
        } => ("1".into(), None),
        ParameterExpr::Substring {
            indirect: false,
            offset,
            length,
            ..
        } => (
            offset.value.clone(),
            length.as_ref().map(|value| value.value.clone()),
        ),
        _ => return None,
    };
    Some(PositionalList {
        offset,
        length,
        concatenate: *concatenate,
        quoted,
    })
}

pub(super) fn first_literal(raw: &str) -> Option<String> {
    let pieces = word::parse(raw, &ParserOptions::default()).ok()?;
    match &pieces.first()?.piece {
        WordPiece::Text(text) => Some(text.clone()),
        _ => None,
    }
}

pub(super) fn without_leading_parameter(raw: &str, name: &str) -> Option<String> {
    let pieces = word::parse(raw, &ParserOptions::default()).ok()?;
    let first = pieces.first()?;
    let first = match &first.piece {
        WordPiece::DoubleQuotedSequence(inner) => inner.first()?,
        _ => first,
    };
    if !matches!(&first.piece, WordPiece::ParameterExpansion(ParameterExpr::Parameter {
        parameter: Parameter::Named(found), indirect: false,
    }) if found == name)
    {
        return None;
    }
    Some(format!(
        "{}{}",
        &raw[..first.start_index],
        &raw[first.end_index..]
    ))
}

pub(super) fn parameter_affixes(raw: &str, name: &str) -> Option<(String, String, bool)> {
    fn literal_parts(
        pieces: &[WordPieceWithSource],
        name: &str,
        quoted: bool,
        found: &mut bool,
        split: &mut bool,
        prefix: &mut String,
        suffix: &mut String,
    ) -> Option<()> {
        for piece in pieces {
            let text = match &piece.piece {
                WordPiece::DoubleQuotedSequence(inner) => {
                    literal_parts(inner, name, true, found, split, prefix, suffix)?;
                    continue;
                }
                WordPiece::ParameterExpansion(expr)
                    if matches!(
                        expr,
                        ParameterExpr::Parameter {
                            indirect: false,
                            ..
                        }
                    ) && parameter_name(expr).as_deref() == Some(name)
                        && !*found =>
                {
                    *found = true;
                    *split = !quoted;
                    continue;
                }
                WordPiece::Text(text) => text.replace("\\\n", ""),
                WordPiece::SingleQuotedText(text) => text.clone(),
                WordPiece::EscapeSequence(text) if text == "\\\n" => String::new(),
                WordPiece::EscapeSequence(text) => text.strip_prefix('\\').unwrap_or(text).into(),
                _ => return None,
            };
            if *found {
                suffix.push_str(&text);
            } else {
                prefix.push_str(&text);
            }
        }
        Some(())
    }
    let pieces = word::parse(raw, &ParserOptions::default()).ok()?;
    let mut found = false;
    let mut split = false;
    let (mut prefix, mut suffix) = (String::new(), String::new());
    literal_parts(
        &pieces,
        name,
        false,
        &mut found,
        &mut split,
        &mut prefix,
        &mut suffix,
    )?;
    found.then_some((prefix, suffix, split))
}

pub(super) fn single_literal(raw: &str) -> Option<String> {
    let pieces = word::parse(raw, &ParserOptions::default()).ok()?;
    pieces
        .iter()
        .all(|piece| {
            matches!(
                piece.piece,
                WordPiece::Text(_) | WordPiece::TildeExpansion(_) | WordPiece::EscapeSequence(_)
            )
        })
        .then(|| raw.to_owned())
}

#[derive(Clone, Copy)]
pub(super) struct ExpansionContext<'a> {
    pub named_dirs: &'a BTreeMap<String, Option<String>>,
    pub zsh: bool,
    pub assignments: &'a [(String, String)],
    pub variables: &'a BTreeMap<String, String>,
    pub unknown_variables: &'a std::collections::BTreeSet<String>,
    pub deadline: Option<std::time::Instant>,
    pub runtime_variables: &'a std::collections::BTreeSet<String>,
    pub pattern_variables: &'a BTreeMap<String, Vec<std::ops::Range<usize>>>,
    pub host: crate::record::HostFacts<'a>,
    pub cwd: &'a str,
    pub tilde_assigned: bool,
}

impl ExpansionContext<'_> {
    fn get(&self, name: &str) -> Option<&String> {
        self.assignments
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
            .or_else(|| self.variables.get(name))
    }
}

// The seed remains live while nested sources are observed. Return its payload
// off the recursive frame so the supported nesting frontier fits the host stack.
pub(super) fn expand_boxed(
    raw: &str,
    syntax: &super::WordSyntax,
    context: &ExpansionContext<'_>,
) -> Result<Box<Expanded>, CheckError> {
    expand(raw, syntax, context).map(Box::new)
}

pub(super) fn expand(
    raw: &str,
    syntax: &super::WordSyntax,
    context: &ExpansionContext<'_>,
) -> Result<Expanded, CheckError> {
    if matches!(syntax, super::WordSyntax::Literal) {
        let word = Word::literal(raw.to_owned());
        return Ok(Expanded {
            named_tildes: std::collections::BTreeSet::new(),
            modifiers: false,
            unset_parameters: std::collections::BTreeSet::new(),
            unknown_splitting: false,
            positional: false,
            split: vec![word.clone()],
            word,
            nested: Vec::new(),
            arithmetic: Vec::new(),
            references: Vec::new(),
            assignments: Vec::new(),
            tilde: false,
            parameters: Vec::new(),
            unsupported: false,
            lexical_ranges: Vec::new(),
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
    let mut out = Expanded {
        named_tildes: std::collections::BTreeSet::new(),
        modifiers: false,
        unset_parameters: std::collections::BTreeSet::new(),
        unknown_splitting: false,
        positional: false,
        word: Word::literal(String::new()),
        split: Vec::new(),
        nested: Vec::new(),
        arithmetic: Vec::new(),
        references: Vec::new(),
        assignments: Vec::new(),
        tilde: false,
        parameters: Vec::new(),
        unsupported: false,
        lexical_ranges: Vec::new(),
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
    // Brush's word parser has no heredoc context inside command substitutions.
    // Mask inert bodies at identical byte offsets; the lexer and nested command
    // observation continue to use the original source.
    let mut parser_input = input.as_bytes().to_vec();
    for range in lexical.quoted_heredoc_ranges() {
        for byte in &mut parser_input[range] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    }
    mask_background_defaults(&input, &lexical, &mut parser_input);
    let parser_input = String::from_utf8(parser_input).map_err(|_| CheckError {
        kind: CheckErrorKind::GuardFault,
    })?;
    let pieces = if heredoc {
        word::parse_heredoc(&parser_input, &options)
    } else {
        word::parse(&parser_input, &options)
    }
    .map_err(|_| CheckError {
        kind: CheckErrorKind::GuardFault,
    })?;
    out.parameters = parameter_regions(&input, &lexical);
    fill(&input, &pieces, &lexical, context, &mut out, &mut splitting)?;
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
    if splitting && context.runtime_variables.contains("IFS") {
        out.word.expands = true;
        out.word.runtime_unknown = true;
    }
    out.split = if splitting {
        fields(
            &out.word.text,
            &out.lexical_ranges,
            context.variables.get("IFS").map(String::as_str),
        )
        .into_iter()
        .map(|text| {
            let mut word = out.word.clone();
            word.text = text.to_owned();
            word.value = word.text.clone();
            // Splitting yields slices of this buffer, so their offsets
            // preserve the cwd-origin ranges without a second text search.
            let start = text.as_ptr() as usize - out.word.text.as_ptr() as usize;
            let end = start + text.len();
            word.cwd_ranges = out
                .word
                .cwd_ranges
                .iter()
                .filter_map(|range| {
                    if range.start >= start && range.end <= end {
                        Some(range.start - start..range.end - start)
                    } else {
                        None
                    }
                })
                .collect();
            word.quoted_ranges = out
                .word
                .quoted_ranges
                .iter()
                .filter_map(|range| {
                    let left = range.start.max(start);
                    let right = range.end.min(end);
                    (left < right).then(|| left - start..right - start)
                })
                .collect();
            if out.word.cwd_ranges.iter().any(|range| {
                range.start < end
                    && range.end > start
                    && !(range.start >= start && range.end <= end)
            }) {
                out.unsupported = true;
            }
            word
        })
        .collect()
    } else {
        vec![out.word.clone()]
    };
    Ok(out)
}

pub(super) fn fields<'a>(
    text: &'a str,
    lexical: &[std::ops::Range<usize>],
    ifs: Option<&str>,
) -> Vec<&'a str> {
    let delimiter = |at: usize, ch: char| {
        ifs.map_or_else(|| ch.is_whitespace(), |ifs| ifs.contains(ch))
            && !lexical.iter().any(|range| range.contains(&at))
    };
    let whitespace = |ch: char| {
        if ifs.is_some() {
            matches!(ch, ' ' | '\t' | '\n')
        } else {
            ch.is_whitespace()
        }
    };
    let mut characters = text.char_indices().peekable();
    let skip_whitespace = |characters: &mut std::iter::Peekable<std::str::CharIndices<'a>>| {
        while characters
            .peek()
            .is_some_and(|&(at, ch)| delimiter(at, ch) && whitespace(ch))
        {
            characters.next();
        }
    };
    let mut output = Vec::new();
    skip_whitespace(&mut characters);
    while let Some(&(start, _)) = characters.peek() {
        while characters
            .peek()
            .is_some_and(|&(at, ch)| !delimiter(at, ch))
        {
            characters.next();
        }
        let end = characters.peek().map_or(text.len(), |&(at, _)| at);
        output.push(&text[start..end]);
        skip_whitespace(&mut characters);
        if characters
            .peek()
            .is_some_and(|&(at, ch)| delimiter(at, ch) && !whitespace(ch))
        {
            characters.next();
        }
        skip_whitespace(&mut characters);
    }
    output
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
    expansion: &ExpansionContext<'_>,
    parameter_word: bool,
) -> Result<Expanded, CheckError> {
    let mut out = Expanded {
        named_tildes: std::collections::BTreeSet::new(),
        modifiers: false,
        unset_parameters: std::collections::BTreeSet::new(),
        unknown_splitting: false,
        positional: false,
        word: Word::literal(String::new()),
        split: Vec::new(),
        nested: Vec::new(),
        arithmetic: Vec::new(),
        references: Vec::new(),
        assignments: Vec::new(),
        tilde: false,
        parameters: Vec::new(),
        unsupported: false,
        lexical_ranges: Vec::new(),
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
    let mut parser_input = raw.as_bytes().to_vec();
    mask_background_defaults(raw, &lexical, &mut parser_input);
    let parser_input = String::from_utf8(parser_input).map_err(|_| CheckError {
        kind: CheckErrorKind::GuardFault,
    })?;
    let pieces = if matches!(
        context.quote,
        super::lexer::Quote::Double | super::lexer::Quote::Gettext
    ) {
        word::parse_heredoc(&parser_input, &ParserOptions::default())
    } else {
        word::parse(&parser_input, &ParserOptions::default())
    };
    match pieces {
        Ok(mut pieces) => {
            // A default/alternative word has its own initial tilde expansion,
            // even when the surrounding parameter expression is double quoted.
            // Keep the surrounding quote context for every other piece.
            if parameter_word
                && raw.starts_with('~')
                && let Ok(unquoted) = word::parse(raw, &ParserOptions::default())
                && let Some(prefix) = unquoted.first()
                && matches!(prefix.piece, WordPiece::TildeExpansion(_))
                && let Some(first) = pieces.first_mut()
                && let WordPiece::Text(text) = &mut first.piece
                && let Some(tail) = text.strip_prefix(&raw[..prefix.end_index])
            {
                *text = tail.into();
                first.start_index = prefix.end_index;
                pieces.insert(0, prefix.clone());
            }
            fill(raw, &pieces, &lexical, expansion, &mut out, &mut false)?;
        }
        Err(_) => out.unsupported = true,
    }
    out.unsupported |= out.parameters.iter().any(|r| !r.supported);
    Ok(out)
}

fn mask_background_defaults(raw: &str, lexical: &super::lexer::Lexed<'_>, input: &mut [u8]) {
    // Brush treats ! as an indirect-name prefix even for the background PID.
    // A supported special parameter preserves its grammar and byte offsets;
    // evaluation still uses the original, runtime-derived spelling.
    for (start, _) in raw.match_indices("${!:") {
        if lexical.context(start).active() {
            input[start + 2] = b'?';
        }
    }
}

fn merge_fragment(out: &mut Expanded, inner: Expanded, offset: usize) {
    out.unsupported |= inner.unsupported;
    out.word.runtime_unknown |= inner.word.runtime_unknown;
    for region in inner.parameters {
        if let Some(parent) = out
            .parameters
            .iter_mut()
            .find(|parent| parent.range.start == offset + region.range.start)
        {
            parent.supported = region.supported;
        }
    }
    out.modifiers |= inner.modifiers;
    out.named_tildes.extend(inner.named_tildes);
    out.unset_parameters.extend(inner.unset_parameters);
    out.word.vars.extend(inner.word.vars);
    out.nested.extend(inner.nested);
    out.arithmetic.extend(inner.arithmetic);
    out.references.extend(inner.references);
    out.tilde |= inner.tilde;
}

fn push_tilde(out: &mut Expanded, value: &str) {
    let start = out.word.text.len();
    out.word.text.push_str(value);
    out.lexical_ranges.push(start..out.word.text.len());
    out.word.quoted_ranges.push(start..out.word.text.len());
}

fn fill(
    raw: &str,
    pieces: &[WordPieceWithSource],
    lexical: &super::lexer::Lexed<'_>,
    expansion: &ExpansionContext<'_>,
    out: &mut Expanded,
    splitting: &mut bool,
) -> Result<(), CheckError> {
    let variables = expansion.variables;
    let host = expansion.host;
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
                let start = out.word.text.len();
                out.word
                    .text
                    .push_str(&raw[covered..piece.end_index].replace("\\\n", ""));
                if quoted {
                    out.word.quoted_ranges.push(start..out.word.text.len());
                } else {
                    out.word.globs |= raw[covered..piece.end_index].contains(['*', '?', '[', '(']);
                }
            } else {
                out.unsupported = true;
            }
            continue;
        }
        let start = out.word.text.len();
        let mut inherited_pattern = false;
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
                fill(raw, inner, lexical, expansion, out, splitting)?
            }
            WordPiece::EscapeSequence(text) => {
                let text = text.strip_prefix('\\').unwrap_or(text);
                if text != "\n" {
                    out.word.text.push_str(text);
                }
            }
            WordPiece::TildeExpansion(tilde) => {
                if let TildeExpr::UserHome(user) = tilde {
                    out.named_tildes.insert(user.clone());
                    if expansion.zsh {
                        if let Some(value) = expansion.named_dirs.get(user) {
                            if let Some(value) = value {
                                push_tilde(out, value.trim_end_matches('/'));
                            } else {
                                out.word.text.push_str(spelling);
                                out.word.expands = true;
                            }
                            continue;
                        }
                        out.word.vars.push(user.clone());
                        if let Some(value) =
                            expansion.get(user).filter(|value| value.starts_with('/'))
                            && !expansion.unknown_variables.contains(user)
                        {
                            push_tilde(out, value.trim_end_matches('/'));
                            continue;
                        }
                        if expansion.unknown_variables.contains(user) {
                            out.word.expands = true;
                        }
                    } else {
                        out.word.vars.push(user.clone());
                    }
                }
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
                        out.tilde = true;
                        out.word.vars.push("PWD".into());
                        if expansion.tilde_assigned && variables.contains_key("PWD") {
                            variables.get("PWD").map(String::as_str)
                        } else {
                            out.word.pwd = true;
                            out.word.cwd_ranges.push(
                                out.word.text.len()..out.word.text.len() + expansion.cwd.len(),
                            );
                            Some(expansion.cwd)
                        }
                    }
                    _ => None,
                };
                push_tilde(out, value.unwrap_or(spelling));
            }
            WordPiece::ParameterExpansion(expr) => {
                let modifier =
                    modifiers::chain(raw, expr, lexical, piece.start_index, piece.end_index);
                out.modifiers |= modifier.is_some();
                if expansion.zsh
                    && let Some(modifier) = modifier
                {
                    out.word.vars.push(modifier.name.clone());
                    let value = out
                        .assignments
                        .iter()
                        .rev()
                        .find(|(name, _)| name == &modifier.name)
                        .map(|(_, value)| value.as_str())
                        .or_else(|| expansion.get(&modifier.name).map(String::as_str))
                        .or_else(|| (modifier.name == "PWD").then_some(expansion.cwd));
                    if !expansion.unknown_variables.contains(&modifier.name)
                        && let Some(value) = value
                    {
                        if let Some(value) = modifier.apply(value.into(), expansion)? {
                            out.word.globs |= !quoted && value.contains(['*', '?', '[']);
                            out.word.text.push_str(&value);
                            *splitting |= !quoted;
                        } else {
                            out.unsupported = true;
                        }
                    } else {
                        out.word
                            .text
                            .push_str(&raw[piece.start_index..modifier.end]);
                        out.word.expands = true;
                        out.unknown_splitting |= !quoted;
                    }
                    if let Some(region) = out
                        .parameters
                        .iter_mut()
                        .find(|region| region.range.start == piece.start_index)
                    {
                        region.supported = true;
                    }
                    if quoted {
                        out.word.quoted_ranges.push(start..out.word.text.len());
                        out.lexical_ranges.push(start..out.word.text.len());
                    }
                    covered = modifier.end;
                    continue;
                }
                if let Some(Parameter::NamedWithIndex { index, .. }) = parameter(expr) {
                    out.references.push(index.clone());
                }
                if let ParameterExpr::Substring { offset, length, .. } = expr {
                    out.references.push(offset.value.clone());
                    if let Some(length) = length {
                        out.references.push(length.value.clone());
                    }
                }
                let plain = match expr {
                    ParameterExpr::Parameter {
                        parameter: Parameter::Named(name),
                        indirect: false,
                    } => Some(name.clone()),
                    ParameterExpr::Parameter {
                        parameter: Parameter::Positional(index),
                        indirect: false,
                    } => Some(index.to_string()),
                    ParameterExpr::Parameter {
                        parameter:
                            Parameter::Special(word::SpecialParameter::PositionalParameterCount),
                        indirect: false,
                    } => Some("#".into()),
                    ParameterExpr::Parameter {
                        parameter: Parameter::NamedWithIndex { name, index },
                        indirect: false,
                    } if index.parse::<usize>().is_ok() => Some(format!("{name}[{index}]")),
                    _ => None,
                };
                if let Some(name) = parameter_name(expr) {
                    if matches!(
                        expr,
                        ParameterExpr::UseDefaultValues { .. }
                            | ParameterExpr::AssignDefaultValues { .. }
                            | ParameterExpr::UseAlternativeValue { .. }
                    ) {
                        out.unset_parameters.insert(name.clone());
                    }
                    out.word.vars.push(name);
                }
                if let Some(name) = &plain
                    && !out.word.vars.contains(name)
                {
                    out.word.vars.push(name.clone());
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
                let mut fragments = Vec::new();
                let mut fragment_assignments = expansion.assignments.to_vec();
                fragment_assignments.extend(out.assignments.clone());
                for body in parameter_fragments(expr) {
                    let context = super::lexer::Context {
                        quote: if matches!(
                            expr,
                            ParameterExpr::ReplaceSubstring { .. }
                                | ParameterExpr::RemoveSmallestPrefixPattern { .. }
                                | ParameterExpr::RemoveLargestPrefixPattern { .. }
                                | ParameterExpr::RemoveSmallestSuffixPattern { .. }
                                | ParameterExpr::RemoveLargestSuffixPattern { .. }
                                | ParameterExpr::UppercaseFirstChar { .. }
                                | ParameterExpr::UppercasePattern { .. }
                                | ParameterExpr::LowercaseFirstChar { .. }
                                | ParameterExpr::LowercasePattern { .. }
                        ) {
                            super::lexer::Quote::Unquoted
                        } else {
                            context.quote
                        },
                        parameter_depth: 1,
                        ..super::lexer::Context::default()
                    };
                    let offset = if let Some(offset) = spelling.find(body) {
                        offset
                    } else {
                        let mut parser_spelling = spelling.as_bytes().to_vec();
                        let fragment_lexical = super::lexer::Lexed::parameter_fragment(
                            spelling,
                            super::lexer::Context::default(),
                        )
                        .0;
                        mask_background_defaults(spelling, &fragment_lexical, &mut parser_spelling);
                        let parser_spelling =
                            String::from_utf8(parser_spelling).map_err(|_| CheckError {
                                kind: CheckErrorKind::GuardFault,
                            })?;
                        parser_spelling.find(body).ok_or(CheckError {
                            kind: CheckErrorKind::GuardFault,
                        })?
                    };
                    let inner = fragment(
                        &spelling[offset..offset + body.len()],
                        context,
                        &ExpansionContext {
                            assignments: &fragment_assignments,
                            ..*expansion
                        },
                        matches!(
                            expr,
                            ParameterExpr::UseDefaultValues { .. }
                                | ParameterExpr::AssignDefaultValues { .. }
                                | ParameterExpr::UseAlternativeValue { .. }
                        ),
                    )?;
                    fragment_assignments.extend(inner.assignments.clone());
                    fragments.push((
                        body,
                        parameters::Fragment {
                            value: (!inner.word.expands
                                && !inner.word.runtime_unknown
                                && !inner.unsupported
                                && !inner
                                    .word
                                    .vars
                                    .iter()
                                    .any(|name| expansion.unknown_variables.contains(name)))
                            .then(|| inner.word.text.clone()),
                            pattern: crate::filesystem::shell_pattern(
                                &inner.word.text,
                                &inner.word.quoted_ranges,
                            ),
                            assignments: inner.assignments.clone(),
                        },
                    ));
                    merge_fragment(out, inner, piece.start_index + offset);
                }
                let computed = if plain.is_none() {
                    parameters::value(expr, expansion, &fragments, out)?
                } else {
                    None
                };
                if let Some(value) = computed {
                    out.word.text.push_str(&value);
                    *splitting |= !quoted;
                    out.word.globs |= !quoted && value.contains(['*', '?', '[']);
                } else if let Some(value) = plain.as_deref().and_then(|name| {
                    out.assignments
                        .iter()
                        .rev()
                        .find(|(key, _)| key == name)
                        .map(|(_, value)| value.as_str())
                        .or_else(|| {
                            expansion
                                .get(name)
                                .map(String::as_str)
                                .or_else(|| (name == "HOME").then_some(host.home))
                                .or_else(|| (name == "PWD").then_some(expansion.cwd))
                        })
                }) {
                    if let Some(ranges) = plain
                        .as_ref()
                        .and_then(|name| expansion.pattern_variables.get(name))
                    {
                        // A captured pathname expansion keeps its original
                        // pattern domain through a later quoted reference.
                        out.word.quoted_ranges.extend(
                            ranges
                                .iter()
                                .map(|range| start + range.start..start + range.end),
                        );
                        inherited_pattern = true;
                    }
                    if plain.as_deref() == Some("PWD") && !variables.contains_key("PWD") {
                        out.word.pwd = true;
                        out.word
                            .cwd_ranges
                            .push(out.word.text.len()..out.word.text.len() + value.len());
                    }
                    if plain
                        .as_ref()
                        .is_some_and(|name| expansion.runtime_variables.contains(name))
                    {
                        out.word.runtime_unknown = true;
                        out.unknown_splitting |= !quoted;
                        // Lexical representatives describe unknown output;
                        // their whitespace is not a runtime field boundary.
                        out.lexical_ranges
                            .push(out.word.text.len()..out.word.text.len() + value.len());
                    }
                    out.word.text.push_str(value);
                    *splitting |= !quoted;
                    // Keep Bash pathname expansion even when Zsh leaves the binding literal.
                    out.word.globs |= !quoted && value.contains(['*', '?', '[']);
                } else {
                    out.word.text.push_str(spelling);
                    out.word.expands = true;
                    out.unknown_splitting |= !quoted;
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
                    out.word
                        .cwd_ranges
                        .push(out.word.text.len()..out.word.text.len() + expansion.cwd.len());
                    out.word.text.push_str(expansion.cwd);
                } else if right + 1 == piece.end_index
                    && let Some(value) = dirname_output(code, expansion)?
                {
                    out.word.text.push_str(&value);
                } else {
                    out.word.text.push_str(&raw[piece.start_index..right + 1]);
                    out.word.expands = true;
                    out.unknown_splitting |= !quoted;
                }
            }
            WordPiece::ArithmeticExpression(expr) => {
                out.unknown_splitting |= !quoted;
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
                    expansion,
                    false,
                )?;
                let offset = spelling.find(&expr.value).ok_or(CheckError {
                    kind: CheckErrorKind::GuardFault,
                })?;
                merge_fragment(out, inner, piece.start_index + offset);
                out.word.text.push_str(spelling);
                out.word.expands = true;
            }
        }
        if quoted
            || matches!(
                piece.piece,
                WordPiece::SingleQuotedText(_)
                    | WordPiece::AnsiCQuotedText(_)
                    | WordPiece::EscapeSequence(_)
            )
        {
            out.lexical_ranges.push(start..out.word.text.len());
            if !inherited_pattern
                && !matches!(
                    piece.piece,
                    WordPiece::DoubleQuotedSequence(_) | WordPiece::GettextDoubleQuotedSequence(_)
                )
            {
                out.word.quoted_ranges.push(start..out.word.text.len());
            }
        }
    }
    Ok(())
}

fn dirname_output(
    source: &str,
    context: &ExpansionContext<'_>,
) -> Result<Option<String>, CheckError> {
    if !source.trim_start().starts_with("dirname") {
        return Ok(None);
    }
    let parsed = super::brush::records(source, source)?;
    let Some(records) = parsed.records else {
        return Ok(None);
    };
    let [
        super::Statement::Command {
            assignments,
            argv,
            redirects,
            ..
        },
    ] = records.as_slice()
    else {
        return Ok(None);
    };
    if !assignments.is_empty()
        || !redirects.is_empty()
        || argv.len() != 2
        || argv[0].raw != "dirname"
    {
        return Ok(None);
    }
    let path = expand(&argv[1].raw, &argv[1].syntax, context)?;
    if path.word.expands
        || path.word.runtime_unknown
        || path.word.globs
        || !path.nested.is_empty()
        || !path.arithmetic.is_empty()
        || path.unsupported
        || path.split.len() != 1
        || path.word.starts_with('-')
        || path.word.is_empty()
        || !path.word.vars.is_empty()
    {
        return Ok(None);
    }
    // dirname is lexical; realpath depends on filesystem identity and stays unknown.
    let path = path.word.trim_end_matches('/');
    let parent = path
        .rsplit_once('/')
        .map_or(".", |(parent, _)| parent.trim_end_matches('/'));
    Ok(Some(
        if parent.is_empty() { "/" } else { parent }.to_owned(),
    ))
}

pub(super) fn parameter(expr: &ParameterExpr) -> Option<&Parameter> {
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
            // Hex accepts one or two digits, Unicode exactly four, and octal up to three.
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
