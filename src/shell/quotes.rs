// Raw shell scanners skip quoted spans; decoded words use Brush's pieces instead.
pub(super) fn skip(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let byte = *bytes.get(start)?;
    if byte == b'\\' {
        return Some(start + 1 + source[start + 1..].chars().next().map_or(0, char::len_utf8));
    }
    let dollar = byte == b'$' && matches!(bytes.get(start + 1), Some(b'\'' | b'"'));
    let left = start + usize::from(dollar);
    let quote = *bytes.get(left)?;
    if !matches!(quote, b'\'' | b'"') {
        return None;
    }
    let escapes = quote == b'"' || dollar;
    let mut cursor = left + 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\\' && escapes {
            cursor += 1;
            cursor += source[cursor..].chars().next().map_or(0, char::len_utf8);
        } else if bytes[cursor] == quote {
            return Some(cursor + 1);
        } else {
            cursor += source[cursor..].chars().next().map_or(0, char::len_utf8);
        }
    }
    Some(source.len())
}

pub(super) fn heredoc_delimiter(raw: &str) -> Option<(String, bool)> {
    let mut text = String::new();
    let mut quoted = false;
    let mut cursor = 0;
    while cursor < raw.len() {
        let Some(end) = skip(raw, cursor) else {
            let ch = raw[cursor..].chars().next()?;
            text.push(ch);
            cursor += ch.len_utf8();
            continue;
        };
        quoted = true;
        let part = &raw[cursor..end];
        if let Some(escaped) = part.strip_prefix('\\') {
            if escaped != "\n" {
                text.push_str(escaped);
            }
        } else {
            let dollar = part.starts_with('$');
            let left = usize::from(dollar);
            let quote = part.as_bytes()[left];
            if part.len() < left + 2 || part.as_bytes().last() != Some(&quote) {
                return None;
            }
            let body = &part[left + 1..part.len() - 1];
            if quote == b'\'' {
                if dollar {
                    text.push_str(&super::words::ansi(body));
                } else {
                    text.push_str(body);
                }
            } else {
                let mut chars = body.chars().peekable();
                while let Some(ch) = chars.next() {
                    if ch == '\\'
                        && chars
                            .peek()
                            .is_some_and(|next| matches!(next, '\\' | '"' | '$' | '`' | '\n'))
                    {
                        let next = chars.next()?;
                        if next != '\n' {
                            text.push(next);
                        }
                    } else {
                        text.push(ch);
                    }
                }
            }
        }
        cursor = end;
    }
    Some((text, quoted))
}
