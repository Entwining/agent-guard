// This is the CodeFile token boundary, not language-specific syntax interpretation.
pub fn code_paths(code: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut cursor = 0;
    let mut quote = 0;
    while cursor < code.len() {
        let ch = code[cursor..].chars().next().unwrap_or_default();
        if ch.is_alphanumeric() || matches!(ch, '.' | '/' | '~' | '_' | '$') {
            let start = cursor;
            // Keep the braced HOME prefix in one pathname token.
            if code[cursor..]
                .get(..7)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("${HOME}"))
            {
                cursor += 7;
            } else {
                cursor += ch.len_utf8();
            }
            while cursor < code.len() {
                let next = code[cursor..].chars().next().unwrap_or_default();
                if !(next.is_alphanumeric() || matches!(next, '.' | '/' | '~' | '_' | '-' | '$')) {
                    break;
                }
                cursor += next.len_utf8();
            }
            let token = &code[start..cursor];
            if (start == 0 || code.as_bytes()[start - 1] != b'\\')
                && (token.contains('/')
                    || token.starts_with('.')
                    || quote != 0
                    || matches!(
                        code.as_bytes().get(start.wrapping_sub(1)),
                        Some(b'\'' | b'"' | b'`')
                    )
                    || matches!(code.as_bytes().get(cursor), Some(b'\'' | b'"' | b'`')))
            {
                let home = ["$HOME/", "${HOME}/"].into_iter().find(|prefix| {
                    token
                        .get(..prefix.len())
                        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
                });
                paths.push(home.map_or_else(
                    || token.to_owned(),
                    |prefix| format!("~/{}", &token[prefix.len()..]),
                ));
            }
        } else {
            if matches!(ch, '\'' | '"' | '`')
                && (cursor == 0 || code.as_bytes()[cursor - 1] != b'\\')
            {
                if quote == 0 {
                    quote = ch as u8;
                } else if quote == ch as u8 {
                    quote = 0;
                }
            }
            cursor += ch.len_utf8();
        }
    }
    paths
}
