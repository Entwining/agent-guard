//! The syntax each interpreter language uses to open comments and string
//! content.

use super::Language;
use super::scanner::{Close, Frame, Heredoc, Interpolation, Scanner};

impl Scanner {
    fn starts(&self, at: usize, text: &str) -> bool {
        text.chars()
            .enumerate()
            .all(|(offset, ch)| self.chars.get(at + offset) == Some(&ch))
    }

    /// The end of a comment starting at `at`.
    pub(super) fn comment(&self, at: usize) -> Option<usize> {
        use Language::*;
        let line_end = |from: usize| {
            self.chars[from..]
                .iter()
                .position(|c| *c == '\n')
                .map_or(self.chars.len(), |offset| from + offset)
        };
        let block_end = |from: usize, close: &str| {
            (from..self.chars.len())
                .find(|index| self.starts(*index, close))
                .map_or(self.chars.len(), |index| index + close.chars().count())
        };
        let ch = self.chars[at];
        match self.language {
            Python | Ruby | Php if ch == '#' => Some(line_end(at)),
            // `$#array` is the last index, not a comment.
            Perl if ch == '#' && (at == 0 || self.chars[at - 1] != '$') => Some(line_end(at)),
            JavaScript | Php if self.starts(at, "//") => Some(line_end(at)),
            JavaScript | Php if self.starts(at, "/*") => Some(block_end(at + 2, "*/")),
            Lua if self.starts(at, "--") => {
                let level = self.long_bracket(at + 2);
                Some(match level {
                    Some(level) => block_end(at + 2, &format!("]{}]", "=".repeat(level))),
                    None => line_end(at),
                })
            }
            AppleScript if self.starts(at, "--") || ch == '#' => Some(line_end(at)),
            AppleScript if self.starts(at, "(*") => Some(block_end(at + 2, "*)")),
            _ => None,
        }
    }

    /// The level of a Lua long bracket `[==[` opening at `at`.
    fn long_bracket(&self, at: usize) -> Option<usize> {
        if self.chars.get(at) != Some(&'[') {
            return None;
        }
        let level = self.chars[at + 1..]
            .iter()
            .take_while(|c| **c == '=')
            .count();
        (self.chars.get(at + 1 + level) == Some(&'[')).then_some(level)
    }

    /// The string content that opens at `at`, and where it begins.
    pub(super) fn string_start(&self, at: usize, last: Option<char>) -> Option<(Frame, usize)> {
        use Language::*;
        let ch = self.chars[at];
        let text = |close, interpolation, start| {
            Some((
                Frame::Text {
                    close,
                    interpolation,
                    start,
                },
                start,
            ))
        };
        match (self.language, ch) {
            (Python, '\'' | '"') => {
                // A string prefix of up to two letters, such as `f` or `rb`.
                let prefix = self.chars[..at]
                    .iter()
                    .rev()
                    .take_while(|c| c.is_alphabetic())
                    .count();
                let formatted = prefix <= 2
                    && self.chars[at - prefix..at]
                        .iter()
                        .all(|c| "rRbBuUfF".contains(*c))
                    && self.chars[at - prefix..at]
                        .iter()
                        .any(|c| matches!(c, 'f' | 'F'))
                    && (at == prefix
                        || !self.chars[at - prefix - 1].is_alphanumeric()
                            && self.chars[at - prefix - 1] != '_');
                let interpolation = formatted.then_some(Interpolation::Brace);
                if self.starts(at, &ch.to_string().repeat(3)) {
                    text(Close::Triple(ch), interpolation, at + 3)
                } else {
                    text(Close::Char(ch), interpolation, at + 1)
                }
            }
            (JavaScript, '`') => text(Close::Char('`'), Some(Interpolation::Dollar), at + 1),
            (Ruby, '"' | '`') => text(Close::Char(ch), Some(Interpolation::Hash), at + 1),
            (JavaScript | Ruby | Perl | Php | Lua, '\'' | '"') | (Perl | Php, '`') => {
                text(Close::Char(ch), None, at + 1)
            }
            (AppleScript, '"') => text(Close::Char('"'), None, at + 1),
            (Lua, '[') => {
                let level = self.long_bracket(at)?;
                text(Close::LongBracket(level), None, at + level + 2)
            }
            (Ruby, '%') => {
                let mut next = at + 1;
                let kind = self.chars.get(next).copied()?;
                if "qQwWiIxrs".contains(kind) {
                    next += 1;
                }
                let delimiter = *self.chars.get(next)?;
                if delimiter.is_alphanumeric() || delimiter.is_whitespace() || delimiter == '=' {
                    return None;
                }
                let interpolation =
                    (!"qwis".contains(kind) || next == at + 1).then_some(Interpolation::Hash);
                text(delimited(delimiter, 0)?, interpolation, next + 1)
            }
            (JavaScript | Ruby | Perl, '/') if last.is_none_or(|c| "(,=:[!&|?{};~".contains(c)) => {
                let interpolation = (self.language == Ruby).then_some(Interpolation::Hash);
                text(Close::Regex { class: false }, interpolation, at + 1)
            }
            _ => None,
        }
    }

    /// A Perl quote-like operator such as `qw(...)` or `s/a/b/` named at
    /// `start`.
    pub(super) fn quote_like(&self, start: usize) -> Option<(Frame, usize)> {
        if self.language != Language::Perl {
            return None;
        }
        let end = (start..self.chars.len())
            .find(|index| !self.chars[*index].is_ascii_alphanumeric())
            .unwrap_or(self.chars.len());
        if start > 0 && (self.chars[start - 1].is_alphanumeric() || self.chars[start - 1] == '_') {
            return None;
        }
        let token: String = self.chars[start..end].iter().collect();
        let parts = match token.as_str() {
            "q" | "qq" | "qw" | "qx" | "m" | "qr" => 0,
            "s" | "tr" | "y" => 1,
            _ => return None,
        };
        if start > 0 && "$@%&*>:".contains(self.chars[start - 1]) {
            return None;
        }
        let open = (end..self.chars.len()).find(|index| !self.chars[*index].is_whitespace())?;
        let delimiter = self.chars[open];
        if delimiter.is_alphanumeric()
            || "_,;=)]}>".contains(delimiter)
            || delimiter == '#' && open > end
        {
            return None;
        }
        Some((
            Frame::Text {
                close: delimited(delimiter, parts)?,
                interpolation: None,
                start: open + 1,
            },
            open + 1,
        ))
    }

    /// A Ruby, Perl or PHP heredoc opener at `at`, and where code resumes on
    /// its line.
    pub(super) fn heredoc_start(&self, at: usize) -> Option<(Heredoc, usize)> {
        use Language::*;
        let arrows = match self.language {
            Ruby | Perl => "<<",
            Php => "<<<",
            _ => return None,
        };
        if !self.starts(at, arrows) || self.chars.get(at + arrows.len()) == Some(&'<') {
            return None;
        }
        let mut next = at + arrows.len();
        if self.language != Php && matches!(self.chars.get(next), Some('~' | '-')) {
            next += 1;
        }
        let quote = self
            .chars
            .get(next)
            .copied()
            .filter(|c| matches!(c, '\'' | '"'));
        if quote.is_some() {
            next += 1;
        }
        let name_end = (next..self.chars.len())
            .find(|index| !(self.chars[*index].is_alphanumeric() || self.chars[*index] == '_'))
            .unwrap_or(self.chars.len());
        let first = *self.chars.get(next)?;
        // An unquoted name starts with a letter; Ruby also needs it upper
        // case, since `a <<b` appends.
        if name_end == next
            || first.is_ascii_digit()
            || quote.is_none() && self.language == Ruby && !first.is_uppercase()
        {
            return None;
        }
        let terminator: String = self.chars[next..name_end].iter().collect();
        let mut resume = name_end;
        if let Some(quote) = quote {
            if self.chars.get(resume) != Some(&quote) {
                return None;
            }
            resume += 1;
        }
        let interpolation =
            (quote != Some('\'') && self.language == Ruby).then_some(Interpolation::Hash);
        Some((
            Heredoc {
                terminator,
                interpolation,
            },
            resume,
        ))
    }
}

/// The content that a quote-like delimiter opens.
pub(super) fn delimited(delimiter: char, parts: usize) -> Option<Close> {
    let close = match delimiter {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '<' => '>',
        c if c.is_whitespace() => return None,
        _ => {
            return Some(Close::Repeated { delimiter, parts });
        }
    };
    Some(Close::Bracket {
        open: delimiter,
        close,
        depth: 0,
        parts,
    })
}
