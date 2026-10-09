use super::*;

pub(super) fn brace_text(raw: &str) -> Result<(String, bool), CheckError> {
    let lexical = match crate::shell::lexer::Lexed::scan(raw) {
        Ok(lexical) => lexical,
        Err(crate::shell::lexer::LexError::Unterminated { .. }) => {
            return Ok((raw.to_owned(), false));
        }
        Err(crate::shell::lexer::LexError::Nesting) => {
            return Err(CheckError {
                kind: CheckErrorKind::ResourceLimit,
            });
        }
    };
    let mut text = String::new();
    let mut expands = false;
    let mut cursor = 0;
    while cursor < raw.len() {
        let ch = raw[cursor..].chars().next().unwrap_or_default();
        if lexical.context(cursor).word_syntax()
            && ch == '{'
            && let Some((right, list)) = brace_group(raw, cursor, &lexical)
        {
            let group = &raw[cursor..=right];
            if raw[..cursor].ends_with('$') && lexical.context(cursor - 1).active() {
                text.push_str(group);
            } else {
                let body = &raw[cursor + 1..right];
                let (inner, nested) = brace_text(body)?;
                if let Some(sequence) = brace_sequence(body) {
                    text.push_str(&sequence);
                    expands = true;
                } else {
                    text.push('{');
                    text.push_str(&inner);
                    text.push('}');
                    expands |= nested || list;
                }
            }
            cursor = right + 1;
            continue;
        }
        text.push(ch);
        cursor += ch.len_utf8();
    }
    Ok((text, expands))
}

fn brace_group(
    raw: &str,
    left: usize,
    lexical: &crate::shell::lexer::Lexed<'_>,
) -> Option<(usize, bool)> {
    let mut depth = 0;
    let mut list = false;
    let mut cursor = left;
    while cursor < raw.len() {
        let ch = raw[cursor..].chars().next().unwrap_or_default();
        if lexical.context(cursor).word_syntax() && ch == '{' {
            depth += 1;
        }
        if lexical.context(cursor).word_syntax() && ch == '}' {
            depth -= 1;
            if depth == 0 {
                return Some((cursor, list));
            }
        }
        if lexical.context(cursor).word_syntax() && ch == ',' && depth == 1 {
            list = true;
        }
        cursor += ch.len_utf8();
    }
    None
}

fn brace_sequence(body: &str) -> Option<String> {
    let parts: Vec<_> = body.split("..").collect();
    if !(2..=3).contains(&parts.len()) || parts.get(2).is_some_and(|s| s.parse::<i128>().is_err()) {
        return None;
    }
    if let (Ok(start), Ok(end)) = (parts[0].parse::<i128>(), parts[1].parse::<i128>()) {
        if parts[..2].iter().any(|part| part.starts_with(['+', '-'])) {
            // Signed numeric reach cannot consume an empty string or a letter.
            // A leading plus may also stay literal in zsh (D1/D26).
            let reach = if start == end {
                start.to_string()
            } else {
                "[-0-9]*".into()
            };
            return Some(format!("{{{reach},{{{body}}}}}"));
        }
        return Some("*".into());
    }
    if parts[..2]
        .iter()
        .all(|part| part.len() == 1 && part.as_bytes()[0].is_ascii_alphanumeric())
    {
        // Zsh accepts mixed letter/digit endpoints; Bash may leave them literal.
        return Some("*".into());
    }
    None
}
