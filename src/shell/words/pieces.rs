use super::*;
use brush_parser::word::TildeExpr;

pub(super) fn fill(
    raw: &str,
    pieces: &[WordPieceWithSource],
    lexical: &crate::shell::lexer::Lexed<'_>,
    expansion: &ExpansionContext<'_>,
    out: &mut Expanded,
    splitting: &mut bool,
) -> Result<(), CheckError> {
    let mut covered = 0;
    for piece in pieces {
        let context = lexical.context(piece.start_index);
        let quoted = !context.unquoted() || context.heredoc.is_some();
        // Brush word offsets are UTF-8 byte offsets, unlike its program SourceSpan.
        let spelling = raw
            .get(piece.start_index..piece.end_index)
            .ok_or(CheckError {
                kind: CheckErrorKind::GuardFault,
            })?;
        if piece.end_index <= covered {
            continue;
        }
        if piece.start_index < covered {
            covered_text(raw, covered, piece, quoted, out);
            continue;
        }
        let start = out.word.text.len();
        let mut inherited_pattern = false;
        match &piece.piece {
            WordPiece::Text(text) => {
                out.word.text.push_str(&text.replace("\\\n", ""));
                if !quoted {
                    out.word.globs |= text.contains(['*', '?', '[', '(']);
                }
            }
            WordPiece::SingleQuotedText(text) => out.word.text.push_str(text),
            WordPiece::AnsiCQuotedText(text) => out.word.text.push_str(&ansi(text)),
            WordPiece::DoubleQuotedSequence(inner)
            | WordPiece::GettextDoubleQuotedSequence(inner) => {
                fill(raw, inner, lexical, expansion, out, splitting)?
            }
            WordPiece::EscapeSequence(text) => {
                let text = text.strip_prefix('\\').unwrap_or(text);
                if text != "\n" {
                    out.word.text.push_str(text);
                }
            }
            WordPiece::TildeExpansion(tilde) => {
                if tilde_piece(tilde, spelling, expansion, out) {
                    continue;
                }
            }
            WordPiece::ParameterExpansion(expr) => {
                let (next, inherited) = parameter::parameter_piece(
                    raw, expr, piece, lexical, expansion, out, splitting,
                )?;
                inherited_pattern = inherited;
                if let Some(next) = next {
                    covered = next;
                    continue;
                }
            }
            WordPiece::CommandSubstitution(_) | WordPiece::BackquotedCommandSubstitution(_) => {
                let (next, complete) =
                    substitution_piece(raw, piece, lexical, expansion, out, quoted)?;
                if let Some(next) = next {
                    covered = next;
                }
                if !complete {
                    continue;
                }
            }
            WordPiece::ArithmeticExpression(expr) => {
                arithmetic_piece(raw, expr, piece, lexical, expansion, out, quoted)?
            }
        }
        quote_piece(piece, quoted, inherited_pattern, start, out);
    }
    Ok(())
}

mod parameter;

fn tilde_piece(
    tilde: &TildeExpr,
    spelling: &str,
    expansion: &ExpansionContext<'_>,
    out: &mut Expanded,
) -> bool {
    let variables = expansion.variables;
    let host = expansion.host;
    if let TildeExpr::UserHome(user) = tilde {
        out.named_tildes.insert(user.clone());
        if expansion.zsh {
            out.word.vars.push(user.clone());
            if let Some(value) = expansion.get(user).filter(|value| value.starts_with('/'))
                && !expansion.unknown_variables.contains(user)
            {
                push_tilde(out, value.trim_end_matches('/'));
                return true;
            }
            if expansion.unknown_variables.contains(user) {
                // HASH_DIRS can register runtime output. Its spelling
                // is a lexical prefix, never proof of the directory.
                push_tilde(
                    out,
                    expansion
                        .get(user)
                        .map(String::as_str)
                        .unwrap_or(spelling)
                        .trim_end_matches('/'),
                );
                out.word.expands = true;
                out.word.runtime_unknown = true;
                return true;
            }
        } else {
            out.word.vars.push(user.clone());
        }
    }
    let value = match tilde {
        TildeExpr::Home => {
            out.word.vars.push("HOME".into());
            variables
                .get("HOME")
                .map(String::as_str)
                .or(Some(host.home))
        }
        TildeExpr::UserHome(user) if user == host.user.unwrap_or("unknown") => Some(host.home),
        TildeExpr::WorkingDir => {
            out.tilde = true;
            out.word.vars.push("PWD".into());
            if expansion.tilde_assigned && variables.contains_key("PWD") {
                variables.get("PWD").map(String::as_str)
            } else {
                out.word.pwd = true;
                out.word
                    .cwd_ranges
                    .push(out.word.text.len()..out.word.text.len() + expansion.cwd.len());
                Some(expansion.cwd)
            }
        }
        _ => None,
    };
    push_tilde(out, value.unwrap_or(spelling));
    false
}

fn substitution_piece(
    raw: &str,
    piece: &WordPieceWithSource,
    lexical: &crate::shell::lexer::Lexed<'_>,
    expansion: &ExpansionContext<'_>,
    out: &mut Expanded,
    quoted: bool,
) -> Result<(Option<usize>, bool), CheckError> {
    let mut covered = None;
    let Some(body) = lexical.substitution_body(piece.start_index) else {
        out.unsupported = true;
        out.word.expands = true;
        return Ok((None, false));
    };
    let right = body.end;
    let code = &raw[body];
    out.nested.push(code.to_owned());
    if right + 1 != piece.end_index {
        out.unsupported |= right + 1 < piece.end_index;
        covered = Some(right + 1);
        out.word.expands = true;
    }
    if prints_pwd(code) && right + 1 == piece.end_index {
        out.word.pwd = true;
        out.word
            .cwd_ranges
            .push(out.word.text.len()..out.word.text.len() + expansion.cwd.len());
        out.word.text.push_str(expansion.cwd);
    } else if right + 1 == piece.end_index
        && let Some(value) = dirname_output(code, expansion)?
    {
        out.word.text.push_str(&value);
    } else {
        out.word.text.push_str(&raw[piece.start_index..right + 1]);
        out.word.expands = true;
        out.unknown_splitting |= !quoted;
    }
    Ok((covered, true))
}

fn arithmetic_piece(
    raw: &str,
    expr: &brush_parser::ast::UnexpandedArithmeticExpr,
    piece: &WordPieceWithSource,
    lexical: &crate::shell::lexer::Lexed<'_>,
    expansion: &ExpansionContext<'_>,
    out: &mut Expanded,
    quoted: bool,
) -> Result<(), CheckError> {
    let spelling = &raw[piece.start_index..piece.end_index];
    out.unknown_splitting |= !quoted;
    out.arithmetic.push(expr.value.clone());
    let (open, close) = if spelling.starts_with("$[") {
        (b'[', b']')
    } else {
        (b'(', b')')
    };
    out.unsupported |=
        lexical.closing(piece.start_index + 1, open, close) != Some(piece.end_index - 1);
    let inner = fragment(
        &expr.value,
        crate::shell::lexer::Context {
            arithmetic_depth: 1,
            ..crate::shell::lexer::Context::default()
        },
        expansion,
        false,
    )?;
    let offset = spelling.find(&expr.value).ok_or(CheckError {
        kind: CheckErrorKind::GuardFault,
    })?;
    merge_fragment(out, inner, piece.start_index + offset);
    out.word.text.push_str(spelling);
    out.word.expands = true;
    Ok(())
}

fn quote_piece(
    piece: &WordPieceWithSource,
    quoted: bool,
    inherited_pattern: bool,
    start: usize,
    out: &mut Expanded,
) {
    if quoted
        || matches!(
            piece.piece,
            WordPiece::SingleQuotedText(_)
                | WordPiece::AnsiCQuotedText(_)
                | WordPiece::EscapeSequence(_)
        )
    {
        out.lexical_ranges.push(start..out.word.text.len());
        if !inherited_pattern
            && !matches!(
                piece.piece,
                WordPiece::DoubleQuotedSequence(_) | WordPiece::GettextDoubleQuotedSequence(_)
            )
        {
            out.word.quoted_ranges.push(start..out.word.text.len());
        }
    }
}

fn covered_text(
    raw: &str,
    covered: usize,
    piece: &WordPieceWithSource,
    quoted: bool,
    out: &mut Expanded,
) {
    if matches!(piece.piece, WordPiece::Text(_)) {
        let start = out.word.text.len();
        out.word
            .text
            .push_str(&raw[covered..piece.end_index].replace("\\\n", ""));
        if quoted {
            out.word.quoted_ranges.push(start..out.word.text.len());
        } else {
            out.word.globs |= raw[covered..piece.end_index].contains(['*', '?', '[', '(']);
        }
    } else {
        out.unsupported = true;
    }
}
