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
            let literals = [x, y].into_iter().filter_map(|token| {
                if let Token::Literal(ch) = token {
                    Some(*ch)
                } else {
                    None
                }
            });
            if (0..128)
                .filter_map(char::from_u32)
                .chain(literals)
                .any(|ch| accepts(x, ch) && accepts(y, ch))
            {
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

pub(super) fn path(pattern: &str, subject: &str) -> bool {
    let p: Vec<_> = pattern.split('/').collect();
    let s: Vec<_> = subject.split('/').collect();
    let mut pending = vec![(0, 0)];
    let mut seen = HashSet::new();
    while let Some((i, j)) = pending.pop() {
        if !seen.insert((i, j)) {
            continue;
        }
        if i == p.len() {
            if j == s.len() {
                return true;
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
    false
}

pub(super) fn alternatives(pattern: &str, braces: bool) -> Vec<String> {
    let mut result = vec![pattern.to_owned()];
    let mut index = 0;
    while index < result.len() && result.len() < 512 {
        let source = result[index].clone();
        index += 1;
        if braces
            && let Some(left) = source.find('{')
            && let Some(relative) = source[left + 1..].find('}')
        {
            let right = left + 1 + relative;
            let body = &source[left + 1..right];
            if body.contains(',') {
                for part in body.split(',') {
                    let next = format!("{}{}{}", &source[..left], part, &source[right + 1..]);
                    if !result.contains(&next) {
                        result.push(next);
                    }
                }
            }
        }
        if let Some(left) = source.find('(')
            && let Some(relative) = source[left + 1..].find(')')
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
