use std::collections::{BTreeMap, HashSet};

#[derive(Default)]
pub(super) struct Matcher {
    components: BTreeMap<String, BTreeMap<String, bool>>,
    #[cfg(test)]
    pub(super) component_evaluations: usize,
    #[cfg(test)]
    path_states: usize,
}

impl Matcher {
    pub(super) fn component(&mut self, pattern: &str, subject: &str) -> bool {
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
        self.components
            .entry(pattern.into())
            .or_default()
            .insert(subject.into(), result);
        result
    }

    pub(super) fn visible_component(&mut self, pattern: &str, subject: &str, hidden: bool) -> bool {
        (hidden
            || !subject.starts_with('.')
            || pattern.starts_with('.')
            || pattern.starts_with("\\."))
            && self.component(pattern, subject)
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

fn literal_head(chars: &mut std::str::Chars<'_>) -> Option<char> {
    match chars.next()? {
        '*' | '?' | '[' => None,
        '\\' => Some(chars.next().unwrap_or('\\')),
        ch => Some(ch),
    }
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

pub(super) fn component(pattern: &str, subject: &str) -> bool {
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
    fn literal_subject_work_grows_with_width_and_pattern_depth() {
        for width in [16, 32, 64, 128] {
            for depth in [1, 2, 4, 8, 16] {
                let pattern = format!("{}public", "*x".repeat(depth));
                let subject = format!("{}{}public", "y".repeat(width), "x".repeat(depth));
                let mut comparisons = 0;
                assert!(super::component_counted(&pattern, &subject, &mut || {
                    comparisons += 1
                }));
                assert!(
                    comparisons <= 2 * (width + depth + 6),
                    "width={width}, depth={depth}, comparisons={comparisons}"
                );
            }
        }
    }

    #[test]
    fn literal_subject_matching_avoids_the_pattern_product() {
        for size in [64, 128, 256, 512] {
            let subject = format!("{}public", "x".repeat(size));
            let mut comparisons = 0;
            assert!(super::component_counted("*public", &subject, &mut || {
                comparisons += 1
            }));
            assert!(
                comparisons <= subject.chars().count() + 7,
                "size={size}, comparisons={comparisons}"
            );
        }
    }
    #[test]
    fn literal_subject_matching_preserves_pattern_intersection_results() {
        let units = [
            "a",
            "b",
            "*",
            "?",
            "[ab]",
            "[!a]",
            "[a-c]",
            "[[:digit:]]",
            "\\*",
            "[",
            "\\",
            "é",
        ];
        let mut patterns = vec![String::new()];
        patterns.extend(units.iter().map(|s| s.to_string()));
        for first in units {
            for second in units {
                patterns.push(format!("{first}{second}"));
            }
        }
        let mut subjects = vec![String::new()];
        for width in 1..=3 {
            let alphabet = ['a', 'b', 'x', '*', '\\', 'é', '1'];
            for mut index in 0..alphabet.len().pow(width) {
                let mut subject = String::new();
                for _ in 0..width {
                    subject.push(alphabet[index % alphabet.len()]);
                    index /= alphabet.len();
                }
                subjects.push(subject);
            }
        }
        for pattern in patterns {
            for subject in &subjects {
                assert_eq!(
                    super::component(&pattern, subject),
                    super::intersects(&pattern, &super::escape_literal(subject)),
                    "pattern={pattern:?}, subject={subject:?}"
                );
            }
        }
    }
    #[test]
    fn incompatible_path_anchors_skip_parent_states() {
        for size in [8, 16, 32, 64] {
            let subject = format!("/public/{}/data.json", vec!["nested"; size].join("/"));
            let mut matcher = super::Matcher::default();
            assert!(!matcher.path("**/public.pem", &subject));
            assert!(!matcher.path(&subject, "/h/Library/Containers/x"));
            assert_eq!(matcher.path_states, 0, "size={size}");
        }
    }
    #[test]
    fn universal_component_does_not_enumerate_subject_states() {
        for size in [64, 128, 256, 512] {
            let subject = "public路径[*]".repeat(size);
            let mut comparisons = 0;
            for pattern in ["*", "**", "***"] {
                assert!(super::component_counted(pattern, &subject, &mut || {
                    comparisons += 1;
                }));
            }
            assert_eq!(comparisons, 0, "size={size}");
            assert!(!super::component_counted("", &subject, &mut || {}));
        }
    }
    #[test]
    fn incompatible_pattern_anchors_skip_product_states() {
        for size in [64, 128, 256, 512] {
            let pattern = format!("public{}*[0-9].json", "data".repeat(size));
            let mut comparisons = 0;
            for protected in ["*.pem", "*.key", ".env*", "credentials*"] {
                assert!(!super::intersects_counted(&pattern, protected, &mut || {
                    comparisons += 1;
                }));
            }
            assert_eq!(comparisons, 0, "size={size}");
        }
    }
    #[test]
    fn incompatible_literal_anchors_skip_subject_states() {
        for size in [64, 128, 256, 512] {
            let subject = format!("{}.json", "public".repeat(size));
            let mut comparisons = 0;
            for pattern in ["*.pem", "*.key", ".env*", "config[0-9]*"] {
                assert!(!super::component_counted(pattern, &subject, &mut || {
                    comparisons += 1;
                }));
            }
            assert!(comparisons <= 12, "size={size}, comparisons={comparisons}");
            let long_pattern = format!("public{}*.json", "data".repeat(size));
            let mut comparisons = 0;
            for subject in [".env", "containers", "credentials"] {
                assert!(!super::component_counted(
                    &long_pattern,
                    subject,
                    &mut || {
                        comparisons += 1;
                    }
                ));
            }
            assert_eq!(comparisons, 0, "size={size}");
        }
    }
    #[test]
    fn pattern_state_walk_observes_its_deadline() {
        let expired = std::time::Instant::now() - std::time::Duration::from_secs(1);
        assert_eq!(
            super::Matcher::default()
                .path_checked("*public", "x", Some(expired))
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
