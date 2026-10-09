use super::{ExpansionContext, ParameterExpr, parameter};
use brush_parser::word::Parameter;

pub(super) struct Chain {
    pub name: String,
    pub end: usize,
    steps: Vec<(u8, Option<usize>)>,
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
    let mut steps = Vec::new();
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
        let number_start = at;
        if braced && matches!(step, b'h' | b't') {
            while at < boundary && raw.as_bytes()[at].is_ascii_digit() {
                at += 1;
            }
        }
        let number = (at > number_start)
            .then(|| raw[number_start..at].parse::<usize>().ok())
            .flatten();
        steps.push((step, number.filter(|number| *number > 0)));
    }
    if steps.is_empty() || braced && at != boundary {
        return None;
    }
    Some(Chain {
        name: name.clone(),
        end: if braced { end } else { at },
        steps,
    })
}

impl Chain {
    pub fn apply(
        &self,
        mut value: String,
        context: &ExpansionContext<'_>,
    ) -> Result<Option<String>, crate::CheckError> {
        for (step, number) in &self.steps {
            crate::check_deadline(context.deadline)?;
            value = match step {
                b'h' => {
                    if let Some(number) = number {
                        let leading = usize::from(value.starts_with('/'));
                        let pieces: Vec<_> =
                            value.split('/').filter(|piece| !piece.is_empty()).collect();
                        let head = pieces
                            .into_iter()
                            .take(number.saturating_sub(leading))
                            .collect::<Vec<_>>()
                            .join("/");
                        format!("{}{head}", if leading == 1 { "/" } else { "" })
                    } else {
                        let path = value.trim_end_matches('/');
                        path.rsplit_once('/').map_or_else(
                            || if value.starts_with('/') { "/" } else { "." }.into(),
                            |(head, _)| {
                                if head.trim_end_matches('/').is_empty() {
                                    "/"
                                } else {
                                    head.trim_end_matches('/')
                                }
                                .into()
                            },
                        )
                    }
                }
                b't' => {
                    let path = value.trim_end_matches('/');
                    let mut pieces: Vec<_> =
                        path.split('/').filter(|piece| !piece.is_empty()).collect();
                    let from = pieces.len().saturating_sub(number.unwrap_or(1));
                    pieces.drain(..from);
                    pieces.join("/")
                }
                b'r' | b'e' => {
                    let extension = value.rfind('.').filter(|dot| !value[*dot..].contains('/'));
                    if *step == b'r' {
                        extension.map_or_else(|| value.clone(), |at| value[..at].into())
                    } else {
                        extension.map_or_else(String::new, |at| value[at + 1..].into())
                    }
                }
                b'l' => value.to_lowercase(),
                b'u' => value.to_uppercase(),
                b'a' => {
                    let path = if value.starts_with('/') {
                        value.clone()
                    } else {
                        format!("{}/{value}", context.cwd)
                    };
                    crate::filesystem::normalize(&path, "/", "")
                }
                b'q' if value
                    .chars()
                    .all(|ch| ch.is_alphanumeric() || "/._-".contains(ch)) =>
                {
                    value
                }
                b'Q' => {
                    let Some(value) = unquote(&value) else {
                        return Ok(None);
                    };
                    value
                }
                _ => return Ok(None),
            };
        }
        Ok(Some(value))
    }
}

fn unquote(raw: &str) -> Option<String> {
    use brush_parser::word::WordPiece;
    crate::shell::lexer::Lexed::scan(raw).ok()?;
    let pieces = brush_parser::word::parse(raw, &brush_parser::ParserOptions::default()).ok()?;
    let mut pending: Vec<_> = pieces.iter().rev().collect();
    let mut result = String::new();
    while let Some(piece) = pending.pop() {
        match &piece.piece {
            WordPiece::Text(text) | WordPiece::SingleQuotedText(text) => result.push_str(text),
            WordPiece::DoubleQuotedSequence(inner)
            | WordPiece::GettextDoubleQuotedSequence(inner) => pending.extend(inner.iter().rev()),
            WordPiece::AnsiCQuotedText(text) => result.push_str(&super::ansi(text)),
            WordPiece::EscapeSequence(text) => {
                if text != "\\\n" {
                    result.push_str(text.strip_prefix('\\').unwrap_or(text));
                }
            }
            _ => result.push_str(raw.get(piece.start_index..piece.end_index)?),
        }
    }
    Some(result)
}
