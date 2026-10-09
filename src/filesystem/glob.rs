mod alternatives;
pub(super) use alternatives::alternatives;

use std::collections::{BTreeMap, HashSet};

#[derive(Default)]
pub(super) struct Matcher {
    components: BTreeMap<String, BTreeMap<String, bool>>,
    #[cfg(test)]
    pub(super) component_evaluations: usize,
    #[cfg(test)]
    pub(super) component_queries: usize,
    #[cfg(test)]
    path_states: usize,
}

impl Matcher {
    pub(super) fn component(&mut self, pattern: &str, subject: &str) -> bool {
        #[cfg(test)]
        {
            self.component_queries += 1;
        }
        if let Some(result) = self
            .components
            .get(pattern)
            .and_then(|subjects| subjects.get(subject))
        {
            return *result;
        }
        #[cfg(test)]
        {
            self.component_evaluations += 1;
        }
        let result = component(pattern, subject);
        if let Some(subjects) = self.components.get_mut(pattern) {
            subjects.insert(subject.into(), result);
        } else {
            self.components
                .insert(pattern.into(), BTreeMap::from([(subject.into(), result)]));
        }
        result
    }

    pub(super) fn visible_component(&mut self, pattern: &str, subject: &str, hidden: bool) -> bool {
        (hidden
            || !subject.starts_with('.')
            || pattern.starts_with('.')
            || pattern.starts_with("\\."))
            && self.component(pattern, subject)
    }

    pub(super) fn identifies_component(&mut self, pattern: &str, protected: &str) -> bool {
        let fixed = pattern.trim_matches(['*', '?']);
        !fixed.is_empty()
            && (self.component(protected, fixed)
                || self.component(fixed, protected.trim_matches('*')))
    }

    pub(super) fn path(&mut self, pattern: &str, subject: &str) -> bool {
        match self.path_checked(pattern, subject, None) {
            Ok(result) => result,
            Err(_) => unreachable!("a match without a deadline cannot expire"),
        }
    }
}

#[derive(Clone)]
enum Token {
    Literal(char),
    Any,
    Star,
    Class(String),
}

fn tokens(pattern: &str) -> Vec<Token> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut result = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '*' => result.push(Token::Star),
            '?' => result.push(Token::Any),
            '\\' if index + 1 < chars.len() => {
                index += 1;
                result.push(Token::Literal(chars[index]));
            }
            '[' => {
                let start = index;
                index += 1;
                if chars
                    .get(index)
                    .is_some_and(|c| ['!', '^', ']'].contains(c))
                {
                    index += 1;
                }
                while index < chars.len() {
                    if chars[index] == '\\' && index + 1 < chars.len() {
                        index += 2;
                        continue;
                    }
                    if chars[index] == '[' && chars.get(index + 1) == Some(&':') {
                        index += 2;
                        while index + 1 < chars.len()
                            && !(chars[index] == ':' && chars[index + 1] == ']')
                        {
                            index += 1;
                        }
                        index += 2;
                        continue;
                    }
                    if chars[index] == ']' {
                        break;
                    }
                    index += 1;
                }
                if index < chars.len() {
                    result.push(Token::Class(chars[start + 1..index].iter().collect()));
                } else {
                    result.extend(chars[start..].iter().copied().map(Token::Literal));
                    break;
                }
            }
            ch => result.push(Token::Literal(ch)),
        }
        index += 1;
    }
    result
}

fn class_matches(body: &str, ch: char) -> bool {
    let negate = body.starts_with(['!', '^']);
    let body = if negate { &body[1..] } else { body };
    let mut hit = false;
    let mut rest = body;
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix("[:")
            && let Some(end) = tail.find(":]")
        {
            hit |= match &tail[..end] {
                "alnum" => ch.is_ascii_alphanumeric(),
                "alpha" => ch.is_ascii_alphabetic(),
                "ascii" => ch.is_ascii(),
                "blank" => [' ', '\t'].contains(&ch),
                "cntrl" => ch.is_ascii_control(),
                "digit" => ch.is_ascii_digit(),
                "graph" => ch.is_ascii_graphic(),
                "lower" => ch.is_ascii_lowercase(),
                "print" => ch.is_ascii() && !ch.is_ascii_control(),
                "punct" => ch.is_ascii_punctuation(),
                "space" => ch.is_ascii_whitespace(),
                "upper" => ch.is_ascii_uppercase(),
                "word" => ch.is_ascii_alphanumeric() || ch == '_',
                "xdigit" => ch.is_ascii_hexdigit(),
                _ => false,
            };
            rest = &tail[end + 2..];
            continue;
        }
        let Some((first, consumed)) = class_character(rest) else {
            break;
        };
        let after = &rest[consumed..];
        if let Some(after) = after.strip_prefix('-')
            && let Some((last, consumed)) = class_character(after)
        {
            hit |= first <= ch && ch <= last;
            rest = &after[consumed..];
        } else {
            hit |= first == ch;
            rest = after;
        }
    }
    hit != negate
}

fn class_character(text: &str) -> Option<(char, usize)> {
    let mut chars = text.chars();
    let first = chars.next()?;
    if first == '\\'
        && let Some(next) = chars.next()
    {
        Some((next, first.len_utf8() + next.len_utf8()))
    } else {
        Some((first, first.len_utf8()))
    }
}

fn accepts(token: &Token, ch: char) -> bool {
    match token {
        Token::Literal(value) => *value == ch,
        Token::Any | Token::Star => true,
        Token::Class(body) => class_matches(body, ch),
    }
}

pub(super) fn intersects(left: &str, right: &str) -> bool {
    intersects_counted(left, right, &mut || {})
}

fn literal_head(chars: &mut std::str::Chars<'_>) -> Option<char> {
    match chars.next()? {
        '*' | '?' | '[' => None,
        '\\' => Some(chars.next().unwrap_or('\\')),
        ch => Some(ch),
    }
}

pub(super) fn literal_prefix(pattern: &str) -> (String, &str) {
    let mut chars = pattern.chars();
    let mut prefix = String::new();
    loop {
        let rest = chars.as_str();
        match literal_head(&mut chars) {
            Some(ch) => prefix.push(ch),
            None => return (prefix, rest),
        }
    }
}

pub(super) fn recursive_wildcard(pattern: &str) -> bool {
    tokens(pattern)
        .windows(2)
        .any(|pair| matches!(pair, [Token::Star, Token::Star]))
}

fn literal_last(pattern: &str) -> Option<char> {
    pattern
        .chars()
        .next_back()
        .filter(|ch| !['*', '?', '[', ']', '\\'].contains(ch))
}

fn intersects_counted(left: &str, right: &str, comparisons: &mut impl FnMut()) -> bool {
    if literal_last(left)
        .zip(literal_last(right))
        .is_some_and(|(x, y)| x != y)
    {
        return false;
    }
    let mut l = left.chars();
    let mut r = right.chars();
    while let (Some(x), Some(y)) = (literal_head(&mut l), literal_head(&mut r)) {
        if x != y {
            return false;
        }
    }
    let left = tokens(left);
    let right = tokens(right);
    let mut pending = vec![(0, 0)];
    let mut seen = HashSet::new();
    while let Some((i, j)) = pending.pop() {
        if !seen.insert((i, j)) {
            continue;
        }
        if i == left.len() && j == right.len() {
            return true;
        }
        let x = left.get(i);
        let y = right.get(j);
        let xs = matches!(x, Some(Token::Star));
        let ys = matches!(y, Some(Token::Star));
        if xs {
            pending.push((i + 1, j));
        }
        if ys {
            pending.push((i, j + 1));
        }
        if let (Some(x), Some(y)) = (x, y) {
            let compatible = match (x, y) {
                (Token::Literal(ch), other) | (other, Token::Literal(ch)) => {
                    comparisons();
                    accepts(other, *ch)
                }
                _ => (0..128).filter_map(char::from_u32).any(|ch| {
                    comparisons();
                    accepts(x, ch) && accepts(y, ch)
                }),
            };
            if compatible {
                let next = (i + usize::from(!xs), j + usize::from(!ys));
                if next != (i, j) {
                    pending.push(next);
                }
            }
        }
    }
    false
}

pub(crate) fn component(pattern: &str, subject: &str) -> bool {
    component_counted(pattern, subject, &mut || {})
}

fn component_counted(pattern: &str, subject: &str, comparisons: &mut impl FnMut()) -> bool {
    if !pattern.is_empty() && pattern.bytes().all(|byte| byte == b'*') {
        return true;
    }
    if let Some(last) = literal_last(pattern)
        && !subject.ends_with(last)
    {
        return false;
    }
    let mut subject_chars = subject.chars();
    let mut pattern_chars = pattern.chars();
    while let Some(ch) = literal_head(&mut pattern_chars) {
        if subject_chars.next() != Some(ch) {
            return false;
        }
    }
    let pattern = tokens(pattern);
    let subject: Vec<_> = subject.chars().collect();
    let (mut i, mut j) = (0, 0);
    let mut star = None;
    while j < subject.len() {
        if matches!(pattern.get(i), Some(Token::Star)) {
            star = Some((i, j));
            i += 1;
        } else if pattern.get(i).is_some_and(|token| {
            comparisons();
            accepts(token, subject[j])
        }) {
            i += 1;
            j += 1;
        } else if let Some((at, consumed)) = star
            && consumed < subject.len()
        {
            let consumed = consumed + 1;
            star = Some((at, consumed));
            i = at + 1;
            j = consumed;
        } else {
            return false;
        }
    }
    pattern[i..]
        .iter()
        .all(|token| matches!(token, Token::Star))
}

pub(super) fn visible_intersects(pattern: &str, protected: &str, hidden: bool) -> bool {
    (hidden
        || !protected.starts_with('.')
        || pattern.starts_with('.')
        || pattern.starts_with("\\."))
        && intersects(pattern, protected)
}

pub(crate) fn escape_literal(subject: &str) -> String {
    subject
        .chars()
        .flat_map(|ch| {
            if ['*', '?', '[', '\\'].contains(&ch) {
                vec!['\\', ch]
            } else {
                vec![ch]
            }
        })
        .collect()
}

pub(crate) fn grep_pattern(root: &str, pattern: &str) -> String {
    // Grep applies separator-free globs to basenames at every depth.
    let recursive = if pattern.contains('/') { "" } else { "**/" };
    format!("{}/{recursive}{pattern}", escape_literal(root))
}

pub(crate) fn shell_pattern(text: &str, quoted: &[std::ops::Range<usize>]) -> String {
    let mut pattern = String::new();
    for (at, ch) in text.char_indices() {
        if ch == '\\'
            || matches!(ch, '\'' | '"')
            || quoted.iter().any(|range| range.contains(&at)) && "*?[]{}(),|!^-+@$`:".contains(ch)
        {
            pattern.push('\\');
        }
        pattern.push(ch);
    }
    pattern
}

pub(super) fn shell_syntax(component: &str) -> bool {
    let mut escaped = false;
    component.chars().any(|ch| {
        if escaped {
            escaped = false;
            false
        } else if ch == '\\' {
            escaped = true;
            false
        } else {
            "*?[{($`".contains(ch)
        }
    })
}

impl Matcher {
    pub(super) fn path_checked(
        &mut self,
        pattern: &str,
        subject: &str,
        deadline: Option<std::time::Instant>,
    ) -> Result<bool, crate::CheckError> {
        crate::check_deadline(deadline)?;
        let p: Vec<_> = pattern.split('/').collect();
        let s: Vec<_> = subject.split('/').collect();
        if p.iter().filter(|part| **part != "**").count() > s.len()
            || p.last() != Some(&"**") && !self.component(p[p.len() - 1], s[s.len() - 1])
        {
            return Ok(false);
        }
        let mut pending = vec![(0, 0)];
        let mut seen = HashSet::new();
        while let Some((i, j)) = pending.pop() {
            crate::check_deadline(deadline)?;
            if !seen.insert((i, j)) {
                continue;
            }
            #[cfg(test)]
            {
                self.path_states += 1;
            }
            if i == p.len() {
                if j == s.len() {
                    return Ok(true);
                }
                continue;
            }
            if p[i] == "**" {
                pending.push((i + 1, j));
                if j < s.len() {
                    pending.push((i, j + 1));
                }
            } else if j < s.len() && self.component(p[i], s[j]) {
                pending.push((i + 1, j + 1));
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests;
