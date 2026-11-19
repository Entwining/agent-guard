//! Reads interpreter source as code, string content and interpolations.

use std::collections::VecDeque;

use super::syntax::delimited;
use super::{Language, Mention, braced_home, home_relative, path_char};

/// What ends string content.
#[derive(Debug, Clone)]
pub(super) enum Close {
    Char(char),
    Triple(char),
    /// A bracketing delimiter, which nests; `parts` more delimited parts
    /// follow, as in Perl's `s{...}{...}`.
    Bracket {
        open: char,
        close: char,
        depth: usize,
        parts: usize,
    },
    /// A repeated delimiter that ends `parts` more parts, as in `s/a/b/`.
    Repeated {
        delimiter: char,
        parts: usize,
    },
    /// A regular expression literal, which cannot span lines.
    Regex {
        class: bool,
    },
    /// A heredoc body, which ends at a line holding its terminator.
    Heredoc(String),
    /// A Lua long bracket of the given level.
    LongBracket(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Interpolation {
    /// `${...}`
    Dollar,
    /// `#{...}`
    Hash,
    /// `{...}`, with `{{` and `}}` as literal braces.
    Brace,
}

#[derive(Debug, Clone)]
pub(super) enum Frame {
    Code {
        braces: usize,
        interpolation: bool,
    },
    Text {
        close: Close,
        interpolation: Option<Interpolation>,
        start: usize,
    },
}

#[derive(Debug, Clone)]
pub(super) struct Heredoc {
    pub(super) terminator: String,
    pub(super) interpolation: Option<Interpolation>,
}

/// A token with a slash is a mention anywhere; a bare name is one only as a
/// whole word of string content, since in code it is an identifier or a
/// member such as `.env(...)`.
pub(super) struct Scanner {
    pub(super) chars: Vec<char>,
    pub(super) language: Language,
    mentions: Vec<Mention>,
    stack: Vec<Frame>,
    /// Heredocs whose bodies start after the current line.
    pending: VecDeque<Heredoc>,
    /// The last character code processed, which tells a regex literal from
    /// division.
    last: Option<char>,
}

impl Scanner {
    pub(super) fn new(text: &str, language: Language) -> Self {
        Self {
            chars: text.chars().collect(),
            language,
            mentions: Vec::new(),
            stack: vec![Frame::Code {
                braces: 0,
                interpolation: false,
            }],
            pending: VecDeque::new(),
            last: None,
        }
    }

    pub(super) fn scan(mut self) -> Vec<Mention> {
        let mut i = 0;
        while i < self.chars.len() {
            i = match self.stack.last() {
                Some(Frame::Code { .. }) => self.code(i),
                Some(Frame::Text { .. }) => self.text(i),
                None => break,
            };
        }
        // Unterminated content still holds its mentions.
        let end = self.chars.len();
        for frame in std::mem::take(&mut self.stack) {
            if let Frame::Text { start, .. } = frame
                && start < end
            {
                self.segment(start, end);
            }
        }
        self.mentions
    }

    /// Reads code at `i` and returns the index to continue from.
    fn code(&mut self, i: usize) -> usize {
        let ch = self.chars[i];
        if let Some(Frame::Code {
            braces,
            interpolation: true,
        }) = self.stack.last_mut()
        {
            match ch {
                '}' if *braces == 0 => {
                    self.stack.pop();
                    if let Some(Frame::Text { start, .. }) = self.stack.last_mut() {
                        *start = i + 1;
                    }
                    return i + 1;
                }
                '}' => *braces -= 1,
                '{' => *braces += 1,
                _ => {}
            }
        }
        if ch == '\n'
            && let Some(next) = self.next_heredoc(i)
        {
            return next;
        }
        // A comment is prose, read like string content.
        if let Some(end) = self.comment(i) {
            self.segment(i, end);
            return end;
        }
        if let Some((frame, next)) = self.string_start(i, self.last) {
            self.stack.push(frame);
            return next;
        }
        if let Some((heredoc, next)) = self.heredoc_start(i) {
            self.pending.push_back(heredoc);
            return next;
        }
        if let Some((frame, next)) = self.quote_like(i) {
            self.stack.push(frame);
            return next;
        }
        if path_char(ch) && ch != '-' || braced_home(&self.chars, i) {
            let end = self.token_end(i, false);
            let token: String = self.chars[i..end].iter().collect();
            if self.chars.get(i.wrapping_sub(1)) != Some(&'\\') && token.contains('/') {
                self.mentions.push(Mention {
                    text: home_relative(&token),
                    pattern: false,
                });
            }
            self.last = self.chars.get(end - 1).copied();
            return end;
        }
        if !ch.is_whitespace() {
            self.last = Some(ch);
        }
        i + 1
    }

    /// Opens the next pending heredoc body after the line break at `i`.
    fn next_heredoc(&mut self, i: usize) -> Option<usize> {
        let heredoc = self.pending.pop_front()?;
        self.stack.push(Frame::Text {
            close: Close::Heredoc(heredoc.terminator),
            interpolation: heredoc.interpolation,
            start: i + 1,
        });
        Some(i + 1)
    }

    /// Reads string content at `i` and returns the index to continue from.
    fn text(&mut self, i: usize) -> usize {
        let ch = self.chars[i];
        let Some(Frame::Text {
            close,
            interpolation,
            start,
        }) = self.stack.last()
        else {
            return i + 1;
        };
        let (segment, interpolation) = (*start, *interpolation);
        let raw = matches!(close, Close::LongBracket(_));
        if let Some(next) = self.heredoc_end(i, segment) {
            return next;
        }
        if ch == '\\' && !raw {
            return i + 2;
        }
        if interpolation == Some(Interpolation::Brace)
            && matches!(ch, '{' | '}')
            && self.chars.get(i + 1) == Some(&ch)
        {
            return i + 2;
        }
        if self.opens(i, interpolation) {
            self.segment(segment, i);
            self.stack.push(Frame::Code {
                braces: 0,
                interpolation: true,
            });
            return i + if ch == '{' { 1 } else { 2 };
        }
        let (ends, width) = self.closes(i);
        if !ends {
            return i + 1;
        }
        self.segment(segment, i);
        self.close_text(i + width, interpolation)
    }

    /// Ends a heredoc body at a line holding only its terminator, returning
    /// the index after that line.
    fn heredoc_end(&mut self, i: usize, segment: usize) -> Option<usize> {
        let Some(Frame::Text {
            close: Close::Heredoc(terminator),
            ..
        }) = self.stack.last()
        else {
            return None;
        };
        if i != segment && self.chars[i - 1] != '\n' {
            return None;
        }
        let line_end = self.chars[i..]
            .iter()
            .position(|c| *c == '\n')
            .map_or(self.chars.len(), |offset| i + offset);
        let line: String = self.chars[i..line_end].iter().collect();
        let line = line.trim();
        let ends = line == terminator
            || self.language == Language::Php
                && line.strip_prefix(terminator.as_str()).is_some_and(|rest| {
                    !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_')
                });
        if !ends {
            return None;
        }
        self.segment(segment, i);
        self.stack.pop();
        self.last = None;
        Some(self.next_heredoc(line_end).unwrap_or(line_end))
    }

    /// Whether an interpolation opens at `i`.
    fn opens(&self, i: usize, interpolation: Option<Interpolation>) -> bool {
        let ch = self.chars[i];
        let next = self.chars.get(i + 1);
        match interpolation {
            Some(Interpolation::Dollar) => {
                ch == '$' && next == Some(&'{') && !braced_home(&self.chars, i)
            }
            Some(Interpolation::Hash) => ch == '#' && next == Some(&'{'),
            Some(Interpolation::Brace) => ch == '{' && next != Some(&'{'),
            None => false,
        }
    }

    /// Whether the innermost string closes at `i`, and the width of its
    /// closing delimiter.
    fn closes(&mut self, i: usize) -> (bool, usize) {
        let chars = &self.chars;
        let ch = chars[i];
        let Some(Frame::Text { close, .. }) = self.stack.last_mut() else {
            return (false, 1);
        };
        match close {
            Close::Char(quote) => (ch == *quote, 1),
            Close::Triple(quote) => (
                chars
                    .get(i..i + 3)
                    .is_some_and(|run| run.iter().all(|c| c == quote)),
                3,
            ),
            Close::Bracket {
                open, close, depth, ..
            } => {
                if ch == *open {
                    *depth += 1;
                    (false, 1)
                } else if ch == *close && *depth > 0 {
                    *depth -= 1;
                    (false, 1)
                } else {
                    (ch == *close, 1)
                }
            }
            Close::Repeated { delimiter, .. } => (ch == *delimiter, 1),
            Close::Regex { class } => {
                if ch == '\n' {
                    (true, 0)
                } else if *class {
                    *class = ch != ']';
                    (false, 1)
                } else if ch == '[' {
                    *class = true;
                    (false, 1)
                } else {
                    (ch == '/', 1)
                }
            }
            Close::Heredoc(_) => (false, 1),
            Close::LongBracket(level) => (
                ch == ']'
                    && chars
                        .get(i + 1..i + 1 + *level)
                        .is_some_and(|run| run.iter().all(|c| *c == '='))
                    && chars.get(i + 1 + *level) == Some(&']'),
                *level + 2,
            ),
        }
    }

    /// Pops the closed string and opens the next part of a multi-part
    /// quote-like operator, returning the index to continue from.
    fn close_text(&mut self, next: usize, interpolation: Option<Interpolation>) -> usize {
        let following = match self.stack.pop() {
            Some(Frame::Text {
                close: Close::Repeated { delimiter, parts },
                ..
            }) if parts > 0 => Some((
                Close::Repeated {
                    delimiter,
                    parts: parts - 1,
                },
                next,
            )),
            Some(Frame::Text {
                close: Close::Bracket { parts, .. },
                ..
            }) if parts > 0 => self.chars[next..]
                .iter()
                .position(|c| !c.is_whitespace())
                .map(|offset| next + offset)
                .and_then(|open| Some((delimited(self.chars[open], parts - 1)?, open + 1))),
            _ => None,
        };
        self.last = self.chars.get(next.saturating_sub(1)).copied();
        let Some((close, start)) = following else {
            return next;
        };
        self.stack.push(Frame::Text {
            close,
            interpolation,
            start,
        });
        start
    }

    /// The end of a path token starting at `start`; string content also
    /// joins wildcards.
    fn token_end(&self, start: usize, text: bool) -> usize {
        let mut end = if braced_home(&self.chars, start) {
            start + 7
        } else {
            start + 1
        };
        while end < self.chars.len()
            && (path_char(self.chars[end]) || text && self.chars[end] == '*')
        {
            end += 1;
        }
        end
    }

    /// Mentions in string content between `start` and `end`. A bare name
    /// counts as a whole word: neither escaped nor called as `name(`.
    fn segment(&mut self, start: usize, end: usize) {
        let mut i = start;
        while i < end {
            let ch = self.chars[i];
            if path_char(ch) && ch != '-' || ch == '*' || braced_home(&self.chars, i) {
                let token_end = self.token_end(i, true).min(end);
                let token: String = self.chars[i..token_end].iter().collect();
                let escaped = i > start && self.chars[i - 1] == '\\';
                let called = token_end < end && self.chars[token_end] == '(';
                if !escaped && (token.contains('/') || !called) && token.chars().any(|c| c != '*') {
                    self.mentions.push(Mention {
                        pattern: token.contains('*'),
                        text: home_relative(&token),
                    });
                }
                i = token_end;
            } else {
                i += 1;
            }
        }
    }
}
