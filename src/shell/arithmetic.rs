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

pub(super) struct Integer {
    pub value: Option<i64>,
    #[cfg(test)]
    bindings_evaluated: usize,
}

pub(super) fn integer(
    expression: &str,
    bindings: &BTreeMap<String, String>,
    unknown: &std::collections::BTreeSet<String>,
    deadline: Option<std::time::Instant>,
) -> Result<Integer, CheckError> {
    let mut result = Integer {
        value: None,
        #[cfg(test)]
        bindings_evaluated: 0,
    };
    result.value = resolve_integer(
        expression,
        bindings,
        unknown,
        &mut Vec::new(),
        &mut BTreeMap::new(),
        #[cfg(test)]
        &mut result,
        deadline,
    )?;
    Ok(result)
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
mod tests;

fn resolve_integer(
    expression: &str,
    bindings: &BTreeMap<String, String>,
    unknown: &std::collections::BTreeSet<String>,
    running: &mut Vec<String>,
    memo: &mut BTreeMap<String, Option<i64>>,
    #[cfg(test)] stats: &mut Integer,
    deadline: Option<std::time::Instant>,
) -> Result<Option<i64>, CheckError> {
    use brush_parser::ast::{
        ArithmeticExpr as Expr, ArithmeticTarget, BinaryOperator as Binary, UnaryOperator as Unary,
    };
    crate::check_deadline(deadline)?;
    let Ok(tree) = brush_parser::arithmetic::parse(expression) else {
        return Ok(None);
    };
    enum Work<'a> {
        Value(&'a Expr),
        Unary(Unary),
        Right(Binary, &'a Expr),
        Binary(Binary, i64),
        Conditional(&'a Expr, &'a Expr),
    }
    let mut work = vec![Work::Value(&tree)];
    let mut values = Vec::new();
    while let Some(step) = work.pop() {
        crate::check_deadline(deadline)?;
        match step {
            Work::Value(expr) => match expr {
                Expr::Literal(value) => values.push(*value),
                Expr::Reference(ArithmeticTarget::Variable(name)) => {
                    let Some(value) = binding_integer(
                        name,
                        bindings,
                        unknown,
                        running,
                        memo,
                        #[cfg(test)]
                        stats,
                        deadline,
                    )?
                    else {
                        return Ok(None);
                    };
                    values.push(value);
                }
                Expr::UnaryOp(op, value) => {
                    work.push(Work::Unary(*op));
                    work.push(Work::Value(value));
                }
                Expr::BinaryOp(op, left, right) => {
                    work.push(Work::Right(*op, right));
                    work.push(Work::Value(left));
                }
                Expr::Conditional(test, yes, no) => {
                    work.push(Work::Conditional(yes, no));
                    work.push(Work::Value(test));
                }
                _ => return Ok(None),
            },
            Work::Unary(op) => {
                let Some(value) = values.pop() else {
                    return Ok(None);
                };
                values.push(match op {
                    Unary::UnaryPlus => value,
                    Unary::UnaryMinus => value.wrapping_neg(),
                    Unary::BitwiseNot => !value,
                    Unary::LogicalNot => i64::from(value == 0),
                });
            }
            Work::Right(op, right) => {
                let Some(left) = values.pop() else {
                    return Ok(None);
                };
                if matches!(op, Binary::LogicalAnd) && left == 0 {
                    values.push(0);
                } else if matches!(op, Binary::LogicalOr) && left != 0 {
                    values.push(1);
                } else {
                    work.push(Work::Binary(op, left));
                    work.push(Work::Value(right));
                }
            }
            Work::Binary(op, left) => {
                let Some(right) = values.pop() else {
                    return Ok(None);
                };
                let value = binary_integer(op, left, right);
                let Some(value) = value else { return Ok(None) };
                values.push(value);
            }
            Work::Conditional(yes, no) => {
                let Some(test) = values.pop() else {
                    return Ok(None);
                };
                work.push(Work::Value(if test != 0 { yes } else { no }));
            }
        }
    }
    Ok(values.pop())
}

fn binding_integer(
    name: &String,
    bindings: &BTreeMap<String, String>,
    unknown: &std::collections::BTreeSet<String>,
    running: &mut Vec<String>,
    memo: &mut BTreeMap<String, Option<i64>>,
    #[cfg(test)] stats: &mut Integer,
    deadline: Option<std::time::Instant>,
) -> Result<Option<i64>, CheckError> {
    if unknown.contains(name) || running.contains(name) || running.len() >= MAX_NESTING {
        return Ok(None);
    }
    if let Some(value) = memo.get(name) {
        let Some(value) = value else { return Ok(None) };
        return Ok(Some(*value));
    }
    #[cfg(test)]
    {
        stats.bindings_evaluated += 1;
    }
    let value = if let Some(value) = bindings.get(name) {
        running.push(name.clone());
        let result = resolve_integer(
            value,
            bindings,
            unknown,
            running,
            memo,
            #[cfg(test)]
            stats,
            deadline,
        );
        running.pop();
        result?
    } else {
        Some(0)
    };
    memo.insert(name.clone(), value);
    let Some(value) = value else { return Ok(None) };
    Ok(Some(value))
}

fn binary_integer(op: brush_parser::ast::BinaryOperator, left: i64, right: i64) -> Option<i64> {
    use brush_parser::ast::BinaryOperator as Binary;
    match op {
        Binary::Add => Some(left.wrapping_add(right)),
        Binary::Subtract => Some(left.wrapping_sub(right)),
        Binary::Multiply => Some(left.wrapping_mul(right)),
        Binary::Divide => left.checked_div(right),
        Binary::Modulo => left.checked_rem(right),
        Binary::Power => u32::try_from(right)
            .ok()
            .map(|right| left.wrapping_pow(right)),
        Binary::ShiftLeft => Some(left.wrapping_shl(right as u32)),
        Binary::ShiftRight => Some(left.wrapping_shr(right as u32)),
        Binary::BitwiseAnd => Some(left & right),
        Binary::BitwiseOr => Some(left | right),
        Binary::BitwiseXor => Some(left ^ right),
        Binary::LogicalAnd | Binary::LogicalOr => Some(i64::from(right != 0)),
        Binary::Equals => Some(i64::from(left == right)),
        Binary::NotEquals => Some(i64::from(left != right)),
        Binary::LessThan => Some(i64::from(left < right)),
        Binary::LessThanOrEqualTo => Some(i64::from(left <= right)),
        Binary::GreaterThan => Some(i64::from(left > right)),
        Binary::GreaterThanOrEqualTo => Some(i64::from(left >= right)),
        Binary::Comma => Some(right),
    }
}
