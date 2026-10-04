use super::lexer::{Context, Lexed};
use crate::{CheckError, CheckErrorKind, limits::MAX_NESTING};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Evaluation {
    pub code: Vec<String>,
    pub bounded: bool,
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
