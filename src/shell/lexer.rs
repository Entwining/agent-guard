//! Raw shell quotation context, independent of the parser and its success.

use crate::limits::MAX_NESTING;
use std::{collections::VecDeque, ops::Range};

pub(super) fn shell_blank(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n')
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Quote {
    #[default]
    Unquoted,
    Single,
    Double,
    AnsiC,
    Gettext,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    pub quote: Quote,
    pub command_depth: usize,
    pub parameter_depth: usize,
    pub backtick_depth: usize,
    pub arithmetic_depth: usize,
    pub command_syntax: bool,
    pub escaped: bool,
    pub heredoc: Option<bool>,
    pub heredoc_delimiter: bool,
    pub comment: bool,
}

impl Context {
    pub fn active(self) -> bool {
        !self.escaped
            && !self.heredoc_delimiter
            && !self.comment
            && self.heredoc != Some(true)
            && !matches!(self.quote, Quote::Single | Quote::AnsiC)
    }
    pub fn unquoted(self) -> bool {
        self.active() && self.quote == Quote::Unquoted
    }
    pub fn word_syntax(self) -> bool {
        self.unquoted()
            && self.command_depth == 0
            && self.parameter_depth == 0
            && self.backtick_depth == 0
            && self.arithmetic_depth == 0
            && self.heredoc.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexError {
    Unterminated { start: usize },
    Nesting,
}

#[derive(Debug)]
pub struct Lexed<'a> {
    source: &'a str,
    context: Vec<Context>,
    heredocs: Vec<(Range<usize>, bool, bool)>,
}

pub fn initial_quote(source: &str) -> Quote {
    match source.as_bytes() {
        [b'\'', ..] => Quote::Single,
        [b'"', ..] => Quote::Double,
        [b'$', b'\'', ..] => Quote::AnsiC,
        [b'$', b'"', ..] => Quote::Gettext,
        _ => Quote::Unquoted,
    }
}

impl<'a> Lexed<'a> {
    pub fn scan(source: &'a str) -> Result<Self, LexError> {
        Self::with_context(source, Context::default())
    }
    pub(crate) fn parameter_fragment(
        source: &'a str,
        context: Context,
    ) -> (Self, Option<LexError>) {
        let mut lexed = Self {
            source,
            context: vec![context; source.len()],
            heredocs: Vec::new(),
        };
        let mut cursor = 0;
        let error = lexed.region(&mut cursor, context, None, 0).err();
        (lexed, error)
    }
    fn with_context(source: &'a str, context: Context) -> Result<Self, LexError> {
        let (lexed, error) = Self::parameter_fragment(source, context);
        match error {
            Some(error) => Err(error),
            None => Ok(lexed),
        }
    }
    pub fn context(&self, byte: usize) -> Context {
        self.context.get(byte).copied().unwrap_or_default()
    }
    pub(super) fn heredoc_body(&self, start: usize) -> Option<&(Range<usize>, bool, bool)> {
        self.heredocs
            .iter()
            .find(|(range, _, _)| range.start == start)
    }
    pub(super) fn quoted_heredoc_ranges(&self) -> impl Iterator<Item = Range<usize>> + '_ {
        self.heredocs
            .iter()
            .filter(|(_, quoted, _)| *quoted)
            .map(|(range, _, _)| range.clone())
    }
    pub fn substitution_body(&self, start: usize) -> Option<Range<usize>> {
        if !self.context(start).active() {
            return None;
        }
        let tail = self.source.get(start..)?;
        let (left, delimiter) = if tail.starts_with("$(") && !tail.starts_with("$((") {
            (start + 1, b')')
        } else if tail.starts_with('`') {
            (start, b'`')
        } else {
            return None;
        };
        let right = self.closing(left, if delimiter == b')' { b'(' } else { b'`' }, delimiter)?;
        Some(left + 1..right)
    }
    pub fn array_tail_spans(&self) -> Vec<Range<usize>> {
        let mut spans = Vec::new();
        for (left, byte) in self.source.bytes().enumerate() {
            let base = self.context(left);
            if byte != b'('
                || !base.unquoted()
                || base.parameter_depth != 0
                || base.arithmetic_depth != 0
            {
                continue;
            }
            let prefix = &self.source[..left];
            let Some(name) = prefix
                .strip_suffix('=')
                .map(|s| s.strip_suffix('+').unwrap_or(s))
            else {
                continue;
            };
            let start = name
                .rfind(|c: char| c.is_ascii() && shell_blank(c as u8) || ";|&<>()".contains(c))
                .map_or(0, |i| i + 1);
            let name = &name[start..];
            if name.is_empty()
                || !name.bytes().enumerate().all(|(i, b)| {
                    (b.is_ascii_alphabetic() || b == b'_' || (i > 0 && b.is_ascii_digit()))
                        && self.context(start + i).unquoted()
                })
            {
                continue;
            }
            let Some(right) = self.closing(left, b'(', b')') else {
                continue;
            };
            if self
                .source
                .as_bytes()
                .get(right + 1)
                .is_none_or(|b| shell_blank(*b) || b";|&<>)".contains(b))
            {
                continue;
            }
            let mut end = right + 1;
            while end < self.source.len() {
                let context = self.context(end);
                let byte = self.source.as_bytes()[end];
                if context == base {
                    let tail = &self.source[end..];
                    let opening = if tail.starts_with("$(")
                        || tail.starts_with("<(")
                        || tail.starts_with(">(")
                    {
                        Some(end + 1)
                    } else if byte == b'(' {
                        Some(end)
                    } else {
                        None
                    };
                    if let Some(opening) = opening
                        && let Some(closing) = self.closing(opening, b'(', b')')
                    {
                        end = closing + 1;
                        continue;
                    }
                }
                if context == base && (shell_blank(byte) || b";|&<>)".contains(&byte)) {
                    break;
                }
                end = self.next(end);
            }
            spans.push(right..end);
        }
        spans
    }
    pub(super) fn closing(&self, start: usize, open: u8, close: u8) -> Option<usize> {
        let base = self.context(start);
        let mut depth: usize = 0;
        for (offset, byte) in self.source.as_bytes()[start..].iter().copied().enumerate() {
            let context = self.context(start + offset);
            if !context.active()
                || context.quote != base.quote
                || context.command_depth != base.command_depth
                || context.parameter_depth != base.parameter_depth
                || context.backtick_depth != base.backtick_depth
                || context.arithmetic_depth != base.arithmetic_depth
            {
                continue;
            }
            if open == close && byte == close && offset > 0 {
                return Some(start + offset);
            }
            if byte == open {
                depth += 1;
            } else if byte == close {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(start + offset);
                }
            }
        }
        None
    }
}

mod expansion;
mod heredoc;
mod scan;
