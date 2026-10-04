use crate::{CheckError, CheckErrorKind};
use std::ops::Range;

#[derive(Default)]
pub(super) struct Detection {
    pub divergent: bool,
    pub executable_qualifier: bool,
    pub masked: String,
    pub code: Vec<String>,
    pub evaluated_variables: Vec<String>,
    pub parameter_spans: Vec<Range<usize>>,
}

fn mask(bytes: &mut [u8], range: Range<usize>) {
    for byte in &mut bytes[range] {
        *byte = b'_';
    }
}

fn qualifier(body: &str, trailing: bool) -> bool {
    trailing
        && (body.contains('+')
            || body
                .as_bytes()
                .windows(2)
                .any(|pair| pair[0] == b'e' && !pair[1].is_ascii_alphanumeric()))
}

// Detection reads original checked spans even when the Bash parser rejects them.
pub(super) fn detect_lexed(
    source: &str,
    spans: &[Range<usize>],
    lexical: &super::lexer::Lexed<'_>,
) -> Result<Detection, CheckError> {
    for span in spans {
        if source.get(span.clone()).is_none() {
            return Err(CheckError {
                kind: CheckErrorKind::GuardFault,
            });
        }
    }
    let mut result = Detection {
        divergent: !lexical.array_tail_spans().is_empty(),
        masked: source.to_owned(),
        ..Detection::default()
    };
    let mut masked = source.as_bytes().to_vec();
    let mut cursor = 0;
    while cursor < source.len() {
        let tail = &source[cursor..];
        let context = lexical.context(cursor);
        let byte = source.as_bytes()[cursor];
        if !context.active() {
            cursor += source[cursor..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        if (tail.starts_with("${(")
            || ["${~", "${=", "${^"]
                .iter()
                .any(|prefix| tail.starts_with(prefix)))
            && let Some(end) = lexical.closing(cursor + 1, b'{', b'}')
        {
            result.divergent = true;
            result.parameter_spans.push(cursor..end + 1);
            let parameter = &source[cursor + 3..end];
            if tail.starts_with("${(")
                && let Some((flags, name)) = parameter.split_once(')')
                && flags.contains('e')
            {
                result.evaluated_variables.push(name.to_owned());
            }
            mask(&mut masked, cursor..end + 1);
            cursor = end + 1;
            continue;
        }
        if context.unquoted()
            && tail.starts_with("=(")
            && !assignment_prefix(source, cursor, lexical)
            && let Some(end) = lexical.closing(cursor + 1, b'(', b')')
        {
            result.divergent = true;
            result.code.push(source[cursor + 2..end].to_owned());
            mask(&mut masked, cursor..end + 1);
            cursor = end + 1;
            continue;
        }
        if context.unquoted()
            && byte == b'('
            && cursor > 0
            && matches!(
                source.as_bytes()[cursor - 1],
                b'*' | b'?' | b'+' | b'!' | b'@'
            )
            && let Some(end) = lexical.closing(cursor, b'(', b')')
        {
            let body = &source[cursor + 1..end];
            let trailing = end + 1 == source.len()
                || matches!(
                    source.as_bytes()[end + 1],
                    b' ' | b'\t' | b'\n' | b';' | b')' | b'"'
                );
            if qualifier(body, trailing) {
                result.divergent = true;
                result.executable_qualifier = true;
                if let Some(code) = body.strip_prefix("e:").and_then(|s| s.strip_suffix(':')) {
                    let bytes = code.as_bytes();
                    let code = if bytes.len() >= 2
                        && matches!(
                            super::lexer::initial_quote(code),
                            super::lexer::Quote::Single | super::lexer::Quote::Double
                        )
                        && bytes.last() == Some(&bytes[0])
                    {
                        &code[1..code.len() - 1]
                    } else {
                        code
                    };
                    result.code.push(code.to_owned());
                }
            }
            // Ordinary extglob/filter groups retain their original operand spelling.
            mask(&mut masked, cursor..end + 1);
            cursor = end + 1;
            continue;
        }
        if context.unquoted()
            && ["setopt", "unsetopt", "emulate"].iter().any(|name| {
                tail.starts_with(name)
                    && tail
                        .as_bytes()
                        .get(name.len())
                        .is_none_or(u8::is_ascii_whitespace)
            })
            && statement_boundary(&source[..cursor])
        {
            result.divergent = true;
        }
        cursor += source[cursor..].chars().next().map_or(1, char::len_utf8);
    }
    result.masked = String::from_utf8(masked).map_err(|_| CheckError {
        kind: CheckErrorKind::GuardFault,
    })?;
    Ok(result)
}

fn assignment_prefix(source: &str, end: usize, lexical: &super::lexer::Lexed<'_>) -> bool {
    let start = source[..end]
        .rfind(|c: char| c.is_ascii_whitespace() || ";|&()".contains(c))
        .map_or(0, |i| i + 1);
    let name = source[start..end]
        .strip_suffix('+')
        .unwrap_or(&source[start..end]);
    !name.is_empty()
        && name.bytes().enumerate().all(|(i, b)| {
            (b.is_ascii_alphabetic() || b == b'_' || (i > 0 && b.is_ascii_digit()))
                && lexical.context(start + i).unquoted()
        })
}

fn statement_boundary(prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches([' ', '\t']);
    prefix.is_empty()
        || prefix.ends_with([';', '\n', '|', '&', '(', '{'])
        || prefix
            .split_whitespace()
            .last()
            .is_some_and(|word| ["then", "do", "else"].contains(&word))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn detect(source: &str, spans: &[Range<usize>]) -> Result<Detection, CheckError> {
        detect_lexed(
            source,
            spans,
            &super::super::lexer::Lexed::scan(source).unwrap(),
        )
    }
    #[test]
    fn glob_position_rule() {
        assert!(!qualifier("a|b", false));
        assert!(!qualifier(".", true));
        assert!(!qualifier("x", true));
        assert!(!qualifier("extra", true));
        assert!(qualifier("+filter", true));
        assert!(qualifier("e:'cat input':", true));
        assert!(!qualifier("e:'cat input':", false));
    }
    #[test]
    fn original_utf8_spans_are_checked() {
        assert!(detect("路径", std::slice::from_ref(&(1..2))).is_err());
        let inert = "printf '${(f)v}'";
        let active = "printf \"${(f)v}\"";
        assert!(
            !detect(inert, std::slice::from_ref(&(0..inert.len())))
                .unwrap()
                .divergent
        );
        assert!(
            detect(active, std::slice::from_ref(&(0..active.len())))
                .unwrap()
                .divergent
        );
    }
    #[test]
    fn parameter_mask_uses_the_complete_nested_region() {
        let source = "echo ${v:-${(e)${name}}} tail";
        let found = detect(source, std::slice::from_ref(&(0..source.len()))).unwrap();
        assert_eq!(found.masked, "echo ${v:-_____________} tail");
        assert_eq!(found.evaluated_variables, ["${name}"]);
    }
    #[test]
    fn modifier_detection_has_its_own_cause_and_span() {
        for modifier in ["~", "~~", "=", "==", "^", "^^"] {
            for quoted in [false, true] {
                let expansion = format!("${{{modifier}v}}");
                let source = if quoted {
                    format!("echo \"{expansion}\"")
                } else {
                    format!("echo {expansion}")
                };
                let start = source.find("${").unwrap();
                let found = detect(&source, std::slice::from_ref(&(0..source.len()))).unwrap();
                assert!(found.divergent, "D30 modifier cause: {source}");
                assert_eq!(
                    found.parameter_spans,
                    vec![start..start + expansion.len()],
                    "D30 span: {source}"
                );
                assert!(found.code.is_empty() && found.evaluated_variables.is_empty());
            }
        }
        assert!(!detect("echo '${~v}'", &[]).unwrap().divergent);
        assert!(!detect("a=(x y)", &[]).unwrap().divergent);
        assert!(detect("cat =(echo public)", &[]).unwrap().divergent);
    }
    #[test]
    fn parameter_pairing_ignores_braces_in_nested_code() {
        let source = "echo ${~v:-$(echo {)}";
        let found = detect(source, &[]).unwrap();
        assert!(
            found.divergent,
            "D30 modifier still has a complete source span"
        );
        assert_eq!(
            found.parameter_spans,
            std::iter::once(5..source.len()).collect::<Vec<_>>()
        );
    }
}
