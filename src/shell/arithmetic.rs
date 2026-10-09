use super::lexer::{Context, Lexed};
use crate::{CheckError, CheckErrorKind, limits::MAX_NESTING};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Evaluation {
    pub code: Vec<String>,
    pub bounded: bool,
    pub names: Vec<String>,
    visits: usize,
}

// Binding values are arithmetic expressions. Only checked subscript bodies
// become shell source; ordinary variable data never enters the shell parser.
pub(super) fn evaluate(
    expression: &str,
    bindings: &BTreeMap<String, Vec<String>>,
) -> Result<Evaluation, CheckError> {
    let mut result = Evaluation::default();
    visit(expression, bindings, &mut Vec::new(), &mut result, false)?;
    Ok(result)
}

pub(super) enum Arming {
    Inert,
    Armed(Vec<String>),
    Unresolved,
}

pub(super) fn armed(value: &str) -> Arming {
    let (lexical, error) = Lexed::parameter_fragment(
        value,
        Context {
            arithmetic_depth: 1,
            ..Context::default()
        },
    );
    if error == Some(super::lexer::LexError::Nesting) {
        return Arming::Unresolved;
    }
    let mut code = Vec::new();
    for (left, byte) in value.bytes().enumerate() {
        if byte != b'[' || !lexical.context(left).active() {
            continue;
        }
        let mut start = left;
        while start > 0
            && (value.as_bytes()[start - 1].is_ascii_alphanumeric()
                || value.as_bytes()[start - 1] == b'_')
        {
            start -= 1;
        }
        if start == left
            || !(value.as_bytes()[start].is_ascii_alphabetic() || value.as_bytes()[start] == b'_')
        {
            continue;
        }
        let right = lexical.closing(left, b'[', b']');
        let mut offset = left + 1;
        while offset < right.unwrap_or(value.len()) {
            if let Some(body) = lexical.substitution_body(offset) {
                if right.is_none() {
                    return Arming::Unresolved;
                }
                let source = value[body.clone()].to_owned();
                if !code.contains(&source) {
                    code.push(source);
                }
                offset = body.end + 1;
            } else {
                offset += 1;
            }
        }
    }
    if code.is_empty() {
        Arming::Inert
    } else {
        Arming::Armed(code)
    }
}

fn visit(
    expression: &str,
    bindings: &BTreeMap<String, Vec<String>>,
    running: &mut Vec<String>,
    result: &mut Evaluation,
    binding: bool,
) -> Result<(), CheckError> {
    if result.visits == 512 {
        result.bounded = true;
        return Ok(());
    }
    result.visits += 1;
    let (lexical, error) = Lexed::parameter_fragment(
        expression,
        Context {
            arithmetic_depth: 1,
            ..Context::default()
        },
    );
    if error == Some(super::lexer::LexError::Nesting) {
        return Err(CheckError {
            kind: CheckErrorKind::ResourceLimit,
        });
    }
    let mut cursor = 0;
    let mut subscript = 0usize;
    while cursor < expression.len() {
        let byte = expression.as_bytes()[cursor];
        if expression[cursor..].starts_with("${")
            && let Some(end) = lexical.closing(cursor + 1, b'{', b'}')
        {
            let name = &expression[cursor + 2..end];
            if !name
                .bytes()
                .enumerate()
                .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || i > 0 && b.is_ascii_digit())
            {
                cursor = end + 1;
                continue;
            }
        }
        if let Some(body) = lexical.substitution_body(cursor) {
            if !binding || subscript > 0 {
                let code = expression[body.clone()].to_owned();
                if !result.code.contains(&code) {
                    result.code.push(code);
                }
            }
            cursor = body.end + 1;
            continue;
        }
        match byte {
            b'[' => subscript += 1,
            b']' => subscript = subscript.saturating_sub(1),
            _ => {}
        }
        if lexical.context(cursor).active() && (byte.is_ascii_alphabetic() || byte == b'_') {
            let start = cursor;
            while cursor < expression.len()
                && (expression.as_bytes()[cursor].is_ascii_alphanumeric()
                    || expression.as_bytes()[cursor] == b'_')
            {
                cursor += 1;
            }
            let name = &expression[start..cursor];
            if !result.names.iter().any(|n| n == name) {
                result.names.push(name.to_owned());
            }
            if let Some(values) = bindings.get(name) {
                if running.len() >= MAX_NESTING || running.iter().any(|n| n == name) {
                    result.bounded = true;
                    continue;
                }
                running.push(name.to_owned());
                for value in values {
                    visit(value, bindings, running, result, true)?;
                }
                running.pop();
            }
            continue;
        }
        cursor += expression[cursor..]
            .chars()
            .next()
            .ok_or(CheckError {
                kind: CheckErrorKind::GuardFault,
            })?
            .len_utf8();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armed_classifier_refuses_its_own_lexer_bound() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/rust-m2-arith-sinks.json"
        ))
        .unwrap();
        let row = fixture["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "armed-lexer-bound")
            .unwrap();
        let value = row["source"]
            .as_str()
            .unwrap()
            .strip_prefix("x='")
            .unwrap()
            .strip_suffix("'; echo x")
            .unwrap();
        assert!(matches!(armed(value), Arming::Unresolved));
    }
}
