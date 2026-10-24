use super::*;

// The seed remains live while nested sources are observed. Return its payload
// off the recursive frame so the supported nesting frontier fits the host stack.
pub(in crate::shell) fn expand_boxed(
    raw: &str,
    syntax: &crate::shell::WordSyntax,
    context: &ExpansionContext<'_>,
) -> Result<Box<Expanded>, CheckError> {
    expand(raw, syntax, context).map(Box::new)
}

pub(in crate::shell) fn expand(
    raw: &str,
    syntax: &crate::shell::WordSyntax,
    context: &ExpansionContext<'_>,
) -> Result<Expanded, CheckError> {
    if matches!(syntax, crate::shell::WordSyntax::Literal) {
        return Ok(literal_word(raw));
    }
    let heredoc = matches!(syntax, crate::shell::WordSyntax::Heredoc);
    let arithmetic = matches!(syntax, crate::shell::WordSyntax::Arithmetic);
    let options = ParserOptions::default();
    let (input, braces) = if heredoc || arithmetic {
        (raw.to_owned(), false)
    } else {
        brace_text(raw)?
    };
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
        conditional_assignments: Vec::new(),
        tilde: false,
        parameters: Vec::new(),
        unsupported: false,
        lexical_ranges: Vec::new(),
        uncertain_decisions: false,
    };
    out.word.raw = raw.to_owned();
    out.word.globs = braces;
    let mut splitting = false;
    let (lexical, error) = crate::shell::lexer::Lexed::parameter_fragment(
        &input,
        crate::shell::lexer::Context {
            heredoc: heredoc.then_some(false),
            arithmetic_depth: usize::from(arithmetic),
            ..crate::shell::lexer::Context::default()
        },
    );
    match error {
        Some(crate::shell::lexer::LexError::Nesting) => {
            return Err(CheckError {
                kind: CheckErrorKind::ResourceLimit,
            });
        }
        Some(crate::shell::lexer::LexError::Unterminated { .. }) => out.unsupported = true,
        None => {}
    }
    // Brush's word parser has no heredoc context inside command substitutions.
    // Mask inert bodies at identical byte offsets; the lexer and nested command
    // observation continue to use the original source.
    let mut parser_input = input.as_bytes().to_vec();
    for range in lexical.quoted_heredoc_ranges() {
        for byte in &mut parser_input[range] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    }
    mask_background_defaults(&input, &lexical, &mut parser_input);
    let parser_input = String::from_utf8(parser_input).map_err(|_| CheckError {
        kind: CheckErrorKind::GuardFault,
    })?;
    let pieces = if heredoc {
        word::parse_heredoc(&parser_input, &options)
    } else {
        word::parse(&parser_input, &options)
    }
    .map_err(|_| CheckError {
        kind: CheckErrorKind::GuardFault,
    })?;
    out.parameters = parameter_regions(&input, &lexical);
    fill(&input, &pieces, &lexical, context, &mut out, &mut splitting)?;
    out.unsupported |= out.parameters.iter().any(|region| !region.supported);
    if input != raw && !out.parameters.is_empty() {
        original_regions(raw, &mut out.parameters)?;
    }
    if arithmetic {
        out.arithmetic.push(raw.to_owned());
    }
    out.word.value = out.word.text.clone();
    if splitting && context.runtime_variables.contains("IFS") {
        out.word.expands = true;
        out.word.runtime_unknown = true;
        out.word.gaps = vec![crate::record::Gap {
            range: 0..out.word.text.len(),
            nonempty: false,
        }];
        out.unknown_splitting = true;
    }
    out.split = split_fields(&mut out, context, splitting);
    Ok(out)
}

/// Moves parameter regions found in brace-rewritten input back to the byte
/// ranges of the original word.
fn original_regions(
    raw: &str,
    regions: &mut [crate::shell::ParameterRegion],
) -> Result<(), CheckError> {
    let original = crate::shell::lexer::Lexed::scan(raw).map_err(|_| CheckError {
        kind: CheckErrorKind::GuardFault,
    })?;
    let original_regions = parameter_regions(raw, &original);
    if original_regions.len() != regions.len() {
        return Err(CheckError {
            kind: CheckErrorKind::GuardFault,
        });
    }
    for (region, original) in regions.iter_mut().zip(original_regions) {
        region.range = original.range;
    }
    Ok(())
}

fn literal_word(raw: &str) -> Expanded {
    let word = Word::literal(raw.to_owned());
    Expanded {
        named_tildes: std::collections::BTreeSet::new(),
        modifiers: false,
        unset_parameters: std::collections::BTreeSet::new(),
        empty_parameters: std::collections::BTreeSet::new(),
        unknown_splitting: false,
        positional: false,
        split: vec![word.clone()],
        word,
        nested: Vec::new(),
        arithmetic: Vec::new(),
        references: Vec::new(),
        assignments: Vec::new(),
        conditional_assignments: Vec::new(),
        tilde: false,
        parameters: Vec::new(),
        unsupported: false,
        lexical_ranges: Vec::new(),
        uncertain_decisions: false,
    }
}

fn split_fields(out: &mut Expanded, context: &ExpansionContext<'_>, splitting: bool) -> Vec<Word> {
    if splitting {
        fields(
            &out.word.text,
            &out.lexical_ranges,
            context.variables.get("IFS").map(String::as_str),
        )
        .into_iter()
        .map(|text| {
            let mut word = out.word.clone();
            word.text = text.to_owned();
            word.value = word.text.clone();
            // Splitting yields slices of this buffer, so their offsets
            // preserve the cwd-origin ranges without a second text search.
            let start = text.as_ptr() as usize - out.word.text.as_ptr() as usize;
            let end = start + text.len();
            word.cwd_ranges = out
                .word
                .cwd_ranges
                .iter()
                .filter_map(|range| {
                    if range.start >= start && range.end <= end {
                        Some(range.start - start..range.end - start)
                    } else {
                        None
                    }
                })
                .collect();
            word.quoted_ranges = out
                .word
                .quoted_ranges
                .iter()
                .filter_map(|range| {
                    let left = range.start.max(start);
                    let right = range.end.min(end);
                    (left < right).then(|| left - start..right - start)
                })
                .collect();
            // A field cut from an unknown run keeps its part unknown; only an
            // uncut run keeps its nonempty fact.
            word.gaps = out
                .word
                .gaps
                .iter()
                .filter(|gap| gap.range.start <= end && gap.range.end >= start)
                .filter(|gap| !gap.range.is_empty() || (start..=end).contains(&gap.range.start))
                .map(|gap| crate::record::Gap {
                    range: gap.range.start.max(start) - start..gap.range.end.min(end) - start,
                    nonempty: gap.nonempty && gap.range.start >= start && gap.range.end <= end,
                })
                .collect();
            if out.word.cwd_ranges.iter().any(|range| {
                range.start < end
                    && range.end > start
                    && !(range.start >= start && range.end <= end)
            }) {
                out.unsupported = true;
            }
            word
        })
        .collect()
    } else {
        vec![out.word.clone()]
    }
}
