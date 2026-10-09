use super::Expanded;
use crate::{CheckError, CheckErrorKind, record::Word};
use brush_parser::{
    ParserOptions,
    word::{self, Parameter, ParameterExpr, WordPiece, WordPieceWithSource},
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

mod braces;
mod fragments;
mod output;
mod pieces;
use braces::brace_text;
use fragments::{
    fragment, mask_background_defaults, merge_fragment, parameter_regions, push_tilde,
};
pub(super) use output::ansi;
use output::{dirname_output, prints_pwd};
use pieces::fill;

mod expansion;
pub(super) use expansion::{expand, expand_boxed};
