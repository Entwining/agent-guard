use super::{ParameterExpr, parameter};
use brush_parser::word::Parameter;

pub(super) struct Chain {
    pub name: String,
    pub end: usize,
}

pub(super) fn chain(
    raw: &str,
    expr: &ParameterExpr,
    lexical: &crate::shell::lexer::Lexed<'_>,
    start: usize,
    end: usize,
) -> Option<Chain> {
    let Parameter::Named(name) = parameter(expr)? else {
        return None;
    };
    let braced = raw[start..end].starts_with("${");
    let mut at = if braced {
        if !matches!(expr, ParameterExpr::Substring { .. }) {
            return None;
        }
        start + 2 + name.len()
    } else {
        end
    };
    let boundary = if braced { end - 1 } else { raw.len() };
    let mut steps = 0;
    while at + 1 < boundary && raw.as_bytes()[at] == b':' {
        let quote = lexical.context(start).quote;
        if (at..at + 2)
            .any(|at| !lexical.context(at).active() || lexical.context(at).quote != quote)
        {
            break;
        }
        let step = raw.as_bytes()[at + 1];
        if !b"htreulaqQAPc".contains(&step) {
            break;
        }
        at += 2;
        if braced && matches!(step, b'h' | b't') {
            while at < boundary && raw.as_bytes()[at].is_ascii_digit() {
                at += 1;
            }
        }
        steps += 1;
    }
    if steps == 0 || braced && at != boundary {
        return None;
    }
    Some(Chain {
        name: name.clone(),
        end: if braced { end } else { at },
    })
}
