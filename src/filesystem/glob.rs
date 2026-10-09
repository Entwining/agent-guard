use std::collections::HashSet;

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
        let mut chars = rest.char_indices();
        let Some((_, first)) = chars.next() else {
            break;
        };
        if let Some((_, '-')) = chars.next()
            && let Some((end, last)) = chars.next()
        {
            hit |= first <= ch && ch <= last;
            rest = &rest[end + last.len_utf8()..];
        } else {
            hit |= first == ch;
            rest = &rest[first.len_utf8()..];
        }
    }
    hit != negate
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

fn intersects_counted(left: &str, right: &str, comparisons: &mut impl FnMut()) -> bool {
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

pub(super) fn component(pattern: &str, subject: &str) -> bool {
    // Escape the subject so its punctuation never acquires pattern semantics.
    let literal: String = subject
        .chars()
        .flat_map(|ch| {
            if ['*', '?', '[', '\\'].contains(&ch) {
                vec!['\\', ch]
            } else {
                vec![ch]
            }
        })
        .collect();
    intersects(pattern, &literal)
}

pub(super) fn visible_component(pattern: &str, subject: &str, hidden: bool) -> bool {
    (hidden || !subject.starts_with('.') || pattern.starts_with('.') || pattern.starts_with("\\."))
        && component(pattern, subject)
}

pub(super) fn visible_intersects(pattern: &str, protected: &str, hidden: bool) -> bool {
    (hidden
        || !protected.starts_with('.')
        || pattern.starts_with('.')
        || pattern.starts_with("\\."))
        && intersects(pattern, protected)
}

pub(super) fn escape_literal(subject: &str) -> String {
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

pub(super) fn path(pattern: &str, subject: &str) -> bool {
    match path_checked(pattern, subject, None) {
        Ok(result) => result,
        Err(_) => unreachable!("a match without a deadline cannot expire"),
    }
}

pub(super) fn path_checked(
    pattern: &str,
    subject: &str,
    deadline: Option<std::time::Instant>,
) -> Result<bool, crate::CheckError> {
    let p: Vec<_> = pattern.split('/').collect();
    let s: Vec<_> = subject.split('/').collect();
    let mut pending = vec![(0, 0)];
    let mut seen = HashSet::new();
    while let Some((i, j)) = pending.pop() {
        crate::check_deadline(deadline)?;
        if !seen.insert((i, j)) {
            continue;
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
        } else if j < s.len() && component(p[i], s[j]) {
            pending.push((i + 1, j + 1));
        }
    }
    Ok(false)
}

fn brace_members(source: &str) -> Option<(usize, usize, Vec<&str>)> {
    let mut escaped = false;
    for (left, ch) in source.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch != '{' {
            continue;
        }
        let mut depth = 1;
        let mut start = left + 1;
        let mut members = Vec::new();
        let mut escaped = false;
        for (offset, ch) in source[left + 1..].char_indices() {
            let position = left + 1 + offset;
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        if !members.is_empty() {
                            members.push(&source[start..position]);
                            return Some((left, position, members));
                        }
                        break;
                    }
                }
                ',' if depth == 1 => {
                    members.push(&source[start..position]);
                    start = position + 1;
                }
                _ => {}
            }
        }
    }
    None
}

pub(super) fn alternatives(pattern: &str, patterned: bool) -> Vec<String> {
    let mut result = vec![pattern.to_owned()];
    let mut index = 0;
    while index < result.len() && result.len() < 512 {
        let source = result[index].clone();
        index += 1;
        if patterned && let Some((left, right, members)) = brace_members(&source) {
            for part in members {
                let next = format!("{}{}{}", &source[..left], part, &source[right + 1..]);
                if !result.contains(&next) {
                    result.push(next);
                }
            }
        }
        if !patterned || !source.contains('(') {
            continue;
        }
        let lexical = crate::shell::lexer::Lexed::parameter_fragment(
            &source,
            crate::shell::lexer::Context::default(),
        )
        .0;
        if patterned
            && let Some(left) = source.char_indices().find_map(|(at, ch)| {
                (ch == '('
                    && source.as_bytes().get(at.wrapping_sub(1)) != Some(&b'$')
                    && lexical.context(at).command_depth == 0)
                    .then_some(at)
            })
            && let Some(relative) = source[left + 1..].char_indices().find_map(|(at, ch)| {
                (ch == ')' && lexical.context(left + 1 + at).command_depth == 0).then_some(at)
            })
        {
            let right = left + 1 + relative;
            let mut start = left;
            if source[..left].ends_with(['*', '?', '+', '!', '@']) {
                start -= 1;
            }
            let prefix = &source[..start];
            let suffix = &source[right + 1..];
            let body = &source[left + 1..right];
            let directory = prefix.rsplit_once('/').map_or("", |(dir, _)| dir);
            for part in body.split('|') {
                for next in [
                    format!("{prefix}{part}{suffix}"),
                    format!("{directory}/{part}{suffix}"),
                    format!("{}{}", &source[..left], suffix),
                ] {
                    if !result.contains(&next) {
                        result.push(next);
                    }
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod cost {
    #[test]
    fn pattern_state_walk_observes_its_deadline() {
        let expired = std::time::Instant::now() - std::time::Duration::from_secs(1);
        assert_eq!(
            super::path_checked("*public", "x", Some(expired))
                .unwrap_err()
                .kind,
            crate::CheckErrorKind::Deadline
        );
    }
    #[test]
    fn literal_intersection_work_tracks_pattern_length() {
        for size in [64, 128, 256] {
            let left = format!("{}[*].id", "m".repeat(size));
            let right = format!("{}*.id", "m".repeat(size));
            let mut comparisons = 0;
            assert!(super::intersects_counted(&left, &right, &mut || {
                comparisons += 1
            }));
            assert!(
                comparisons <= 4 * (left.len() + right.len()),
                "size={size}, comparisons={comparisons}"
            );
        }
    }
}
