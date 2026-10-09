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

pub(in crate::filesystem) fn alternatives(pattern: &str, patterned: bool) -> Vec<String> {
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
                    && lexical.context(at).word_syntax())
                .then_some(at)
            })
            && let Some(relative) = source[left + 1..].char_indices().find_map(|(at, ch)| {
                (ch == ')' && lexical.context(left + 1 + at).word_syntax()).then_some(at)
            })
        {
            let right = left + 1 + relative;
            let mut start = left;
            if source[..left].ends_with(['*', '?', '+', '!', '@'])
                && lexical.context(left - 1).word_syntax()
            {
                start -= 1;
            }
            let prefix = &source[..start];
            let suffix = &source[right + 1..];
            let body = &source[left + 1..right];
            let directory = prefix.rsplit_once('/').map_or("", |(dir, _)| dir);
            let mut start = 0;
            let mut members = Vec::new();
            for (at, ch) in body.char_indices() {
                if ch == '|' && lexical.context(left + 1 + at).word_syntax() {
                    members.push(&body[start..at]);
                    start = at + 1;
                }
            }
            members.push(&body[start..]);
            for part in members {
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
