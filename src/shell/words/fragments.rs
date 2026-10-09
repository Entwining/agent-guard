use super::*;

pub(super) fn parameter_regions(
    raw: &str,
    lexical: &crate::shell::lexer::Lexed<'_>,
) -> Vec<crate::shell::ParameterRegion> {
    raw.match_indices("${")
        .filter_map(|(start, _)| {
            let context = lexical.context(start);
            (context.active() && context.command_depth == 0 && context.backtick_depth == 0).then(
                || crate::shell::ParameterRegion {
                    range: start
                        ..lexical
                            .closing(start + 1, b'{', b'}')
                            .map_or(raw.len(), |end| end + 1),
                    supported: false,
                },
            )
        })
        .collect()
}

pub(super) fn fragment(
    raw: &str,
    context: crate::shell::lexer::Context,
    expansion: &ExpansionContext<'_>,
    parameter_word: bool,
) -> Result<Expanded, CheckError> {
    let mut out = Expanded {
        named_tildes: std::collections::BTreeSet::new(),
        modifiers: false,
        unset_parameters: std::collections::BTreeSet::new(),
        empty_parameters: std::collections::BTreeSet::new(),
        unknown_splitting: false,
        positional: false,
        word: Word::literal(String::new()),
        split: Vec::new(),
        nested: Vec::new(),
        arithmetic: Vec::new(),
        references: Vec::new(),
        assignments: Vec::new(),
        tilde: false,
        parameters: Vec::new(),
        unsupported: false,
        lexical_ranges: Vec::new(),
    };
    let (lexical, error) = crate::shell::lexer::Lexed::parameter_fragment(raw, context);
    match error {
        Some(crate::shell::lexer::LexError::Nesting) => {
            return Err(CheckError {
                kind: CheckErrorKind::ResourceLimit,
            });
        }
        Some(crate::shell::lexer::LexError::Unterminated { .. }) => out.unsupported = true,
        None => {}
    }
    out.parameters = parameter_regions(raw, &lexical);
    let mut parser_input = raw.as_bytes().to_vec();
    mask_background_defaults(raw, &lexical, &mut parser_input);
    let parser_input = String::from_utf8(parser_input).map_err(|_| CheckError {
        kind: CheckErrorKind::GuardFault,
    })?;
    let pieces = if matches!(
        context.quote,
        crate::shell::lexer::Quote::Double | crate::shell::lexer::Quote::Gettext
    ) {
        word::parse_heredoc(&parser_input, &ParserOptions::default())
    } else {
        word::parse(&parser_input, &ParserOptions::default())
    };
    match pieces {
        Ok(mut pieces) => {
            // A default/alternative word has its own initial tilde expansion,
            // even when the surrounding parameter expression is double quoted.
            // Keep the surrounding quote context for every other piece.
            if parameter_word
                && raw.starts_with('~')
                && let Ok(unquoted) = word::parse(raw, &ParserOptions::default())
                && let Some(prefix) = unquoted.first()
                && matches!(prefix.piece, WordPiece::TildeExpansion(_))
                && let Some(first) = pieces.first_mut()
                && let WordPiece::Text(text) = &mut first.piece
                && let Some(tail) = text.strip_prefix(&raw[..prefix.end_index])
            {
                *text = tail.into();
                first.start_index = prefix.end_index;
                pieces.insert(0, prefix.clone());
            }
            fill(raw, &pieces, &lexical, expansion, &mut out, &mut false)?;
        }
        Err(_) => out.unsupported = true,
    }
    out.unsupported |= out.parameters.iter().any(|r| !r.supported);
    Ok(out)
}

pub(super) fn mask_background_defaults(
    raw: &str,
    lexical: &crate::shell::lexer::Lexed<'_>,
    input: &mut [u8],
) {
    // Brush treats ! as an indirect-name prefix even for the background PID.
    // A supported special parameter preserves its grammar and byte offsets;
    // evaluation still uses the original, runtime-derived spelling.
    for (start, _) in raw.match_indices("${!:") {
        if lexical.context(start).active() {
            input[start + 2] = b'?';
        }
    }
}

pub(super) fn merge_fragment(out: &mut Expanded, inner: Expanded, offset: usize) {
    out.unsupported |= inner.unsupported;
    out.word.runtime_unknown |= inner.word.runtime_unknown;
    for region in inner.parameters {
        if let Some(parent) = out
            .parameters
            .iter_mut()
            .find(|parent| parent.range.start == offset + region.range.start)
        {
            parent.supported = region.supported;
        }
    }
    out.modifiers |= inner.modifiers;
    out.named_tildes.extend(inner.named_tildes);
    out.unset_parameters.extend(inner.unset_parameters);
    out.empty_parameters.extend(inner.empty_parameters);
    out.word.vars.extend(inner.word.vars);
    out.nested.extend(inner.nested);
    out.arithmetic.extend(inner.arithmetic);
    out.references.extend(inner.references);
    out.tilde |= inner.tilde;
}

pub(super) fn push_tilde(out: &mut Expanded, value: &str) {
    let start = out.word.text.len();
    out.word.text.push_str(value);
    out.lexical_ranges.push(start..out.word.text.len());
    out.word.quoted_ranges.push(start..out.word.text.len());
}
