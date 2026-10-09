use super::*;

pub(super) fn dirname_output(
    source: &str,
    context: &ExpansionContext<'_>,
) -> Result<Option<String>, CheckError> {
    if !source.trim_start().starts_with("dirname") {
        return Ok(None);
    }
    let parsed = crate::shell::brush::records(source, source)?;
    let Some(records) = parsed.records else {
        return Ok(None);
    };
    let [
        crate::shell::Statement::Command {
            assignments,
            argv,
            redirects,
            ..
        },
    ] = records.as_slice()
    else {
        return Ok(None);
    };
    if !assignments.is_empty()
        || !redirects.is_empty()
        || argv.len() != 2
        || argv[0].raw != "dirname"
    {
        return Ok(None);
    }
    let path = expand(&argv[1].raw, &argv[1].syntax, context)?;
    if path.word.expands
        || path.word.runtime_unknown
        || path.word.globs
        || !path.nested.is_empty()
        || !path.arithmetic.is_empty()
        || path.unsupported
        || path.split.len() != 1
        || path.word.starts_with('-')
        || path.word.is_empty()
        || !path.word.vars.is_empty()
    {
        return Ok(None);
    }
    // dirname is lexical; realpath depends on filesystem identity and stays unknown.
    let path = path.word.trim_end_matches('/');
    let parent = path
        .rsplit_once('/')
        .map_or(".", |(parent, _)| parent.trim_end_matches('/'));
    Ok(Some(
        if parent.is_empty() { "/" } else { parent }.to_owned(),
    ))
}

pub(super) fn prints_pwd(code: &str) -> bool {
    let words: Vec<_> = code.split_whitespace().collect();
    words == ["pwd"] || words == ["pwd", "-L"] || words == ["pwd", "-P"]
}

pub(in crate::shell) fn ansi(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(ch) = chars.next() else {
            out.push('\\');
            break;
        };
        let (radix, limit) = match ch {
            'x' => (16, 2),
            'u' => (16, 4),
            '0'..='7' => (8, 3),
            _ => (0, 0),
        };
        if radix != 0 {
            let mut digits = String::new();
            if radix == 8 {
                digits.push(ch);
            }
            while digits.len() < limit && chars.peek().is_some_and(|ch| ch.is_digit(radix)) {
                if let Some(ch) = chars.next() {
                    digits.push(ch);
                }
            }
            // Hex accepts one or two digits, Unicode exactly four, and octal up to three.
            if !digits.is_empty() && (ch != 'u' || digits.len() == 4) {
                out.push(
                    u32::from_str_radix(&digits, radix)
                        .ok()
                        .and_then(char::from_u32)
                        .unwrap_or('\u{fffd}'),
                );
                continue;
            }
            out.push(ch);
            out.push_str(&digits);
            continue;
        }
        out.push(match ch {
            'a' => '\u{7}',
            'b' => '\u{8}',
            'e' => '\u{1b}',
            'f' => '\u{c}',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'v' => '\u{b}',
            other => other,
        });
    }
    out
}
