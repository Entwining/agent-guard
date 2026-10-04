//! Raw shell quotation context, independent of the parser and its success.

use crate::limits::MAX_NESTING;
use std::{collections::VecDeque, ops::Range};

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
    pub(super) fn parameter_fragment(
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
                .rfind(|c: char| c.is_ascii_whitespace() || ";|&<>()".contains(c))
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
                .is_none_or(|b| b.is_ascii_whitespace() || b";|&<>)".contains(b))
            {
                continue;
            }
            let mut end = right + 1;
            while end < self.source.len() {
                let context = self.context(end);
                let byte = self.source.as_bytes()[end];
                if context == base && (byte.is_ascii_whitespace() || b";|&<>)".contains(&byte)) {
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
    fn mark(&mut self, range: Range<usize>, context: Context) {
        self.context[range].fill(context);
    }
    fn next(&self, cursor: usize) -> usize {
        cursor
            + self.source[cursor..]
                .chars()
                .next()
                .map_or(0, char::len_utf8)
    }
    fn escape(&mut self, cursor: &mut usize, context: Context) {
        let start = *cursor;
        *cursor += 1;
        if *cursor < self.source.len() {
            *cursor = self.next(*cursor);
        }
        self.mark(
            start..*cursor,
            Context {
                escaped: true,
                ..context
            },
        );
    }
    fn quotation(
        &mut self,
        cursor: &mut usize,
        context: Context,
        quote: Quote,
        depth: usize,
    ) -> Result<(), LexError> {
        let start = *cursor;
        let dollar = matches!(quote, Quote::AnsiC | Quote::Gettext);
        let end = if matches!(quote, Quote::Single | Quote::AnsiC) {
            b'\''
        } else {
            b'"'
        };
        *cursor += 1 + usize::from(dollar);
        let inner = Context { quote, ..context };
        self.mark(start..*cursor, inner);
        if matches!(quote, Quote::Single | Quote::AnsiC) {
            while *cursor < self.source.len() {
                let byte = self.source.as_bytes()[*cursor];
                if byte == end {
                    self.mark(*cursor..*cursor + 1, inner);
                    *cursor += 1;
                    return Ok(());
                }
                if byte == b'\\' && quote == Quote::AnsiC {
                    self.escape(cursor, inner);
                } else {
                    let next = self.next(*cursor);
                    self.mark(*cursor..next, inner);
                    *cursor = next;
                }
            }
        } else {
            self.region(cursor, inner, Some(end), depth + 1)?;
            return Ok(());
        }
        self.unterminated(start).map(|_| ())
    }
    fn unterminated(&self, start: usize) -> Result<Option<usize>, LexError> {
        Err(LexError::Unterminated { start })
    }
    fn region(
        &mut self,
        cursor: &mut usize,
        context: Context,
        end: Option<u8>,
        depth: usize,
    ) -> Result<Option<usize>, LexError> {
        if depth > MAX_NESTING
            || context.command_depth
                + context.parameter_depth
                + context.backtick_depth
                + context.arithmetic_depth
                > MAX_NESTING
        {
            return Err(LexError::Nesting);
        }
        let start = *cursor;
        let mut parens: usize = 0;
        let mut word_start = true;
        let mut word_groups = Vec::new();
        let mut braces: usize = 0;
        let mut documents = VecDeque::new();
        while *cursor < self.source.len() {
            let byte = self.source.as_bytes()[*cursor];
            let tail = &self.source[*cursor..];
            if end == Some(byte) && (end == Some(b'}') || parens == 0) && braces == 0 {
                self.mark(*cursor..*cursor + 1, context);
                *cursor += 1;
                return Ok(Some(*cursor));
            }
            if byte == b'\\'
                && ((context.quote == Quote::Unquoted && context.heredoc.is_none())
                    || self.source.as_bytes().get(*cursor + 1).is_some_and(|next| {
                        matches!(next, b'\\' | b'$' | b'`' | b'\n')
                            || (*next == b'"' && context.heredoc.is_none())
                    }))
            {
                let continuation = self.source.as_bytes().get(*cursor + 1) == Some(&b'\n');
                self.escape(cursor, context);
                if !continuation {
                    word_start = false;
                }
                continue;
            }
            if tail.starts_with("$((") || (context.unquoted() && tail.starts_with("((")) {
                let opening = *cursor;
                let width = if byte == b'$' { 3 } else { 2 };
                self.mark(opening..opening + width, context);
                *cursor += width;
                let inner = Context {
                    quote: Quote::Unquoted,
                    arithmetic_depth: context.arithmetic_depth + 1,
                    command_syntax: false,
                    heredoc: None,
                    ..context
                };
                self.region(cursor, inner, Some(b')'), depth + 1)?;
                if self.source.as_bytes().get(*cursor) != Some(&b')') {
                    self.unterminated(opening)?;
                }
                if *cursor < self.source.len() {
                    self.mark(*cursor - 1..*cursor + 1, context);
                    *cursor += 1;
                }
                word_start = false;
                continue;
            }
            if tail.starts_with("$[") {
                let opening = *cursor;
                self.mark(opening..opening + 2, context);
                *cursor += 2;
                let inner = Context {
                    quote: Quote::Unquoted,
                    arithmetic_depth: context.arithmetic_depth + 1,
                    command_syntax: false,
                    heredoc: None,
                    ..context
                };
                self.region(cursor, inner, Some(b']'), depth + 1)?;
                self.mark(*cursor - 1..*cursor, context);
                word_start = false;
                continue;
            }
            if tail.starts_with("$(") || tail.starts_with("${") || byte == b'`' {
                let opening = *cursor;
                let backtick = byte == b'`';
                let parameter = tail.starts_with("${");
                let width = if backtick { 1 } else { 2 };
                self.mark(opening..opening + width, context);
                *cursor += width;
                let inner = Context {
                    quote: if parameter {
                        context.quote
                    } else {
                        Quote::Unquoted
                    },
                    command_depth: context.command_depth + usize::from(!parameter && !backtick),
                    parameter_depth: context.parameter_depth + usize::from(parameter),
                    command_syntax: !parameter,
                    backtick_depth: context.backtick_depth + usize::from(backtick),
                    heredoc: None,
                    ..context
                };
                self.region(
                    cursor,
                    inner,
                    Some(if backtick {
                        b'`'
                    } else if parameter {
                        b'}'
                    } else {
                        b')'
                    }),
                    depth + 1,
                )?;
                self.mark(*cursor - 1..*cursor, context);
                word_start = false;
                continue;
            }
            if context.heredoc.is_none() && context.quote == Quote::Unquoted {
                let quote = initial_quote(tail);
                if quote != Quote::Unquoted {
                    self.quotation(cursor, context, quote, depth)?;
                    word_start = false;
                    continue;
                }
                if byte == b'#'
                    && word_start
                    && (context.command_syntax
                        || (context.parameter_depth == 0 && context.arithmetic_depth == 0))
                {
                    let right = tail.find('\n').map_or(self.source.len(), |n| *cursor + n);
                    self.mark(
                        *cursor..right,
                        Context {
                            comment: true,
                            ..context
                        },
                    );
                    *cursor = right;
                    continue;
                }
                if tail.starts_with("<<<")
                    && (context.command_syntax
                        || (context.parameter_depth == 0 && context.arithmetic_depth == 0))
                {
                    self.mark(*cursor..*cursor + 3, context);
                    *cursor += 3;
                    word_start = true;
                    continue;
                }
                if tail.starts_with("<<")
                    && (context.command_syntax
                        || (context.parameter_depth == 0 && context.arithmetic_depth == 0))
                {
                    let opening = *cursor;
                    let strip_tabs = tail.starts_with("<<-");
                    *cursor += 2 + usize::from(strip_tabs);
                    self.mark(opening..*cursor, context);
                    while self
                        .source
                        .as_bytes()
                        .get(*cursor)
                        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
                    {
                        self.mark(*cursor..*cursor + 1, context);
                        *cursor += 1;
                    }
                    let delimiter_start = *cursor;
                    let (delimiter, quoted) = self.delimiter(
                        cursor,
                        Context {
                            heredoc_delimiter: true,
                            ..context
                        },
                        depth,
                    )?;
                    word_start = false;
                    if *cursor > delimiter_start && !delimiter.is_empty() {
                        documents.push_back((delimiter, quoted, strip_tabs));
                    }
                    continue;
                }
            }
            if byte == b'\n' && !documents.is_empty() {
                self.mark(*cursor..*cursor + 1, context);
                *cursor += 1;
                while let Some((delimiter, quoted, strip_tabs)) = documents.pop_front() {
                    let body_start = *cursor;
                    let mut line = *cursor;
                    let mut found = None;
                    while line <= self.source.len() {
                        let right = self.source[line..]
                            .find('\n')
                            .map_or(self.source.len(), |n| line + n);
                        let text = &self.source[line..right];
                        if (if strip_tabs {
                            text.trim_start_matches('\t')
                        } else {
                            text
                        }) == delimiter
                        {
                            found = Some((line, right));
                            break;
                        }
                        if right == self.source.len() {
                            break;
                        }
                        line = right + 1;
                    }
                    let body_end = found.map_or(self.source.len(), |(left, _)| left);
                    self.heredocs
                        .push((body_start..body_end, quoted, strip_tabs));
                    let body_context = Context {
                        heredoc: Some(quoted),
                        ..context
                    };
                    self.mark(body_start..body_end, body_context);
                    if !quoted {
                        let body = &self.source[body_start..body_end];
                        let nested = Lexed::with_context(body, body_context)?;
                        self.context[body_start..body_end].copy_from_slice(&nested.context);
                        self.heredocs.extend(nested.heredocs.into_iter().map(
                            |(range, quoted, strip)| {
                                (
                                    range.start + body_start..range.end + body_start,
                                    quoted,
                                    strip,
                                )
                            },
                        ));
                    }
                    *cursor = found.map_or(self.source.len(), |(_, right)| {
                        right + usize::from(right < self.source.len())
                    });
                    if let Some((left, _)) = found {
                        self.mark(
                            left..*cursor,
                            Context {
                                heredoc_delimiter: true,
                                ..context
                            },
                        );
                    }
                }
                continue;
            }
            if context.quote == Quote::Unquoted && context.heredoc.is_none() {
                if byte == b'(' {
                    parens += 1;
                    let process = *cursor > 0
                        && matches!(self.source.as_bytes()[*cursor - 1], b'<' | b'>')
                        && !self.context(*cursor - 1).escaped;
                    word_groups.push(!word_start || process);
                    word_start = true;
                } else if byte == b')' {
                    parens = parens.saturating_sub(1);
                    word_start = !word_groups.pop().unwrap_or(false);
                } else {
                    word_start = byte.is_ascii_whitespace() || b";|&<>".contains(&byte);
                }
                if end == Some(b'}') && byte == b'{' {
                    braces += 1;
                }
                if byte == b'}' {
                    braces = braces.saturating_sub(1);
                }
            }
            let next = self.next(*cursor);
            self.mark(*cursor..next, context);
            *cursor = next;
        }
        if end.is_some() {
            self.unterminated(start)
        } else {
            Ok(None)
        }
    }
    fn delimiter(
        &mut self,
        cursor: &mut usize,
        context: Context,
        depth: usize,
    ) -> Result<(String, bool), LexError> {
        let mut delimiter = String::new();
        let mut quoted = false;
        while *cursor < self.source.len() {
            let byte = self.source.as_bytes()[*cursor];
            if byte.is_ascii_whitespace() || b";|&<>()".contains(&byte) {
                break;
            }
            let quote = initial_quote(&self.source[*cursor..]);
            if quote != Quote::Unquoted {
                quoted = true;
                let start = *cursor;
                self.quotation(cursor, context, quote, depth)?;
                let dollar = matches!(quote, Quote::AnsiC | Quote::Gettext);
                let body = &self.source[start + 1 + usize::from(dollar)..*cursor - 1];
                if quote == Quote::AnsiC {
                    delimiter.push_str(&super::words::ansi(body));
                } else if matches!(quote, Quote::Double | Quote::Gettext) {
                    let mut chars = body.chars().peekable();
                    while let Some(ch) = chars.next() {
                        if ch == '\\'
                            && chars
                                .peek()
                                .is_some_and(|next| matches!(next, '\\' | '"' | '$' | '`' | '\n'))
                        {
                            if let Some(next) = chars.next()
                                && next != '\n'
                            {
                                delimiter.push(next);
                            }
                        } else {
                            delimiter.push(ch);
                        }
                    }
                } else {
                    delimiter.push_str(body);
                }
            } else if byte == b'\\' {
                quoted = true;
                let start = *cursor;
                self.escape(cursor, context);
                let body = &self.source[start + 1..*cursor];
                if body != "\n" {
                    delimiter.push_str(body);
                }
            } else {
                let next = self.next(*cursor);
                delimiter.push_str(&self.source[*cursor..next]);
                self.mark(*cursor..next, context);
                *cursor = next;
            }
        }
        Ok((delimiter, quoted))
    }
}
