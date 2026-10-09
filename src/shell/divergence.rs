use crate::{CheckError, CheckErrorKind};
use std::ops::Range;

#[derive(Default)]
pub(super) struct Detection {
    pub divergent: bool,
    pub executable_qualifier: bool,
    pub masked: String,
    pub code: Vec<String>,
    pub code_regions: Vec<(Range<usize>, String)>,
    pub evaluated_variables: Vec<String>,
    pub evaluated_regions: Vec<(Range<usize>, String)>,
    pub parameter_spans: Vec<Range<usize>>,
    pub array_tail_spans: Vec<Range<usize>>,
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
    let array_tail_spans = lexical.array_tail_spans();
    let mut result = Detection {
        divergent: !array_tail_spans.is_empty(),
        array_tail_spans,
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
                result
                    .evaluated_regions
                    .push((cursor..end + 1, name.to_owned()));
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
            result
                .code_regions
                .push((cursor..end + 1, source[cursor + 2..end].to_owned()));
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
                executable_qualifier(&mut result, body, cursor..end + 1);
            }
            // Ordinary extglob/filter groups retain their original operand spelling.
            mask(&mut masked, cursor..end + 1);
            cursor = end + 1;
            continue;
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
        .rfind(|c: char| c.is_ascii() && super::lexer::shell_blank(c as u8) || ";|&()".contains(c))
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

#[cfg(test)]
mod tests;

fn executable_qualifier(result: &mut Detection, body: &str, range: Range<usize>) {
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
        result.code_regions.push((range, code.to_owned()));
    }
}
