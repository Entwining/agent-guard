use super::*;

pub(super) fn parameter_piece(
    raw: &str,
    expr: &ParameterExpr,
    piece: &WordPieceWithSource,
    lexical: &crate::shell::lexer::Lexed<'_>,
    expansion: &ExpansionContext<'_>,
    out: &mut Expanded,
    splitting: &mut bool,
) -> Result<(Option<usize>, bool), CheckError> {
    let context = lexical.context(piece.start_index);
    let quoted = !context.unquoted() || context.heredoc.is_some();
    let spelling = &raw[piece.start_index..piece.end_index];
    if let Some(end) = modifier_piece(raw, expr, piece, lexical, expansion, out) {
        return Ok((Some(end), false));
    }
    if let Some(Parameter::NamedWithIndex { index, .. }) = parameter(expr) {
        out.references.push(index.clone());
    }
    if let ParameterExpr::Substring { offset, length, .. } = expr {
        out.references.push(offset.value.clone());
        if let Some(length) = length {
            out.references.push(length.value.clone());
        }
    }
    let plain = match expr {
        ParameterExpr::Parameter {
            parameter: Parameter::Named(name),
            indirect: false,
        } => Some(name.clone()),
        ParameterExpr::Parameter {
            parameter: Parameter::Positional(index),
            indirect: false,
        } => Some(index.to_string()),
        ParameterExpr::Parameter {
            parameter: Parameter::Special(word::SpecialParameter::PositionalParameterCount),
            indirect: false,
        } => Some("#".into()),
        ParameterExpr::Parameter {
            parameter: Parameter::NamedWithIndex { name, index },
            indirect: false,
        } if index.parse::<usize>().is_ok() => Some(format!("{name}[{index}]")),
        _ => None,
    };
    if let Some(name) = parameter_name(expr) {
        if matches!(
            expr,
            ParameterExpr::UseDefaultValues { .. }
                | ParameterExpr::AssignDefaultValues { .. }
                | ParameterExpr::UseAlternativeValue { .. }
        ) {
            out.unset_parameters.insert(name.clone());
        }
        out.word.vars.push(name);
    }
    if let Some(name) = &plain
        && !out.word.vars.contains(name)
    {
        out.word.vars.push(name.clone());
    }
    if spelling.starts_with("${")
        && lexical.closing(piece.start_index + 1, b'{', b'}') == Some(piece.end_index - 1)
        && let Some(region) = out
            .parameters
            .iter_mut()
            .find(|r| r.range.start == piece.start_index)
    {
        region.supported = true;
    }
    let fragments = expand_parameter_fragments(expr, spelling, piece, context, expansion, out)?;
    if parameters::needs_empty_context(expr, &fragments, context.parameter_depth > 0)
        && let Some(name) = parameter_name(expr)
    {
        out.empty_parameters.insert(name);
    }
    let computed = if plain.is_none() {
        parameters::value(expr, expansion, &fragments, out)?
    } else {
        None
    };
    let inherited_pattern = parameter_text(
        &plain, computed, spelling, quoted, expansion, out, splitting,
    );
    Ok((None, inherited_pattern))
}

fn modifier_piece(
    raw: &str,
    expr: &ParameterExpr,
    piece: &WordPieceWithSource,
    lexical: &crate::shell::lexer::Lexed<'_>,
    expansion: &ExpansionContext<'_>,
    out: &mut Expanded,
) -> Option<usize> {
    let context = lexical.context(piece.start_index);
    let quoted = !context.unquoted() || context.heredoc.is_some();
    let start = out.word.text.len();
    let modifier = modifiers::chain(raw, expr, lexical, piece.start_index, piece.end_index);
    out.modifiers |= modifier.is_some();
    if expansion.zsh
        && let Some(modifier) = modifier
    {
        out.word.vars.push(modifier.name);
        out.word
            .text
            .push_str(&raw[piece.start_index..modifier.end]);
        out.word.gaps.push(crate::record::Gap {
            range: start..out.word.text.len(),
            nonempty: false,
        });
        out.word.expands = true;
        out.unknown_splitting |= !quoted;
        if let Some(region) = out
            .parameters
            .iter_mut()
            .find(|region| region.range.start == piece.start_index)
        {
            region.supported = true;
        }
        if quoted {
            out.word.quoted_ranges.push(start..out.word.text.len());
            out.lexical_ranges.push(start..out.word.text.len());
        }
        return Some(modifier.end);
    }
    None
}

fn expand_parameter_fragments<'a>(
    expr: &'a ParameterExpr,
    spelling: &str,
    piece: &WordPieceWithSource,
    context: crate::shell::lexer::Context,
    expansion: &ExpansionContext<'_>,
    out: &mut Expanded,
) -> Result<Vec<(&'a str, parameters::Fragment)>, CheckError> {
    let mut fragments = Vec::new();
    let mut fragment_assignments = expansion.assignments.to_vec();
    fragment_assignments.extend(out.assignments.clone());
    for body in parameter_fragments(expr) {
        let context = crate::shell::lexer::Context {
            quote: if matches!(
                expr,
                ParameterExpr::ReplaceSubstring { .. }
                    | ParameterExpr::RemoveSmallestPrefixPattern { .. }
                    | ParameterExpr::RemoveLargestPrefixPattern { .. }
                    | ParameterExpr::RemoveSmallestSuffixPattern { .. }
                    | ParameterExpr::RemoveLargestSuffixPattern { .. }
                    | ParameterExpr::UppercaseFirstChar { .. }
                    | ParameterExpr::UppercasePattern { .. }
                    | ParameterExpr::LowercaseFirstChar { .. }
                    | ParameterExpr::LowercasePattern { .. }
            ) {
                crate::shell::lexer::Quote::Unquoted
            } else {
                context.quote
            },
            parameter_depth: 1,
            ..crate::shell::lexer::Context::default()
        };
        let offset = if let Some(offset) = spelling.find(body) {
            offset
        } else {
            let mut parser_spelling = spelling.as_bytes().to_vec();
            let fragment_lexical = crate::shell::lexer::Lexed::parameter_fragment(
                spelling,
                crate::shell::lexer::Context::default(),
            )
            .0;
            mask_background_defaults(spelling, &fragment_lexical, &mut parser_spelling);
            let parser_spelling = String::from_utf8(parser_spelling).map_err(|_| CheckError {
                kind: CheckErrorKind::GuardFault,
            })?;
            parser_spelling.find(body).ok_or(CheckError {
                kind: CheckErrorKind::GuardFault,
            })?
        };
        let inner = fragment(
            &spelling[offset..offset + body.len()],
            context,
            &ExpansionContext {
                assignments: &fragment_assignments,
                ..*expansion
            },
            matches!(
                expr,
                ParameterExpr::UseDefaultValues { .. }
                    | ParameterExpr::AssignDefaultValues { .. }
                    | ParameterExpr::UseAlternativeValue { .. }
            ),
        )?;
        fragment_assignments.extend(inner.assignments.clone());
        // Pattern characters are fragment syntax, not unknown text.
        let exact = !inner.word.expands
            && !inner.word.runtime_unknown
            && inner.word.gaps.is_empty()
            && !inner.unsupported
            && !inner
                .word
                .vars
                .iter()
                .any(|name| expansion.unknown_variables.contains(name));
        fragments.push((
            body,
            parameters::Fragment {
                value: exact.then(|| inner.word.text.clone()),
                representative: (!inner.unsupported).then(|| inner.word.text.clone()),
                pattern: fragment_pattern(&inner.word, exact),
                assignments: inner.assignments.clone(),
                has_variables: !inner.word.vars.is_empty(),
            },
        ));
        merge_fragment(out, inner, piece.start_index + offset);
    }
    Ok(fragments)
}

/// A fragment's pattern, in which an unknown run may match any text rather
/// than its representative.
fn fragment_pattern(word: &crate::record::Word, exact: bool) -> String {
    let known = |range: std::ops::Range<usize>| {
        let quoted = word
            .quoted_ranges
            .iter()
            .filter_map(|quoted| {
                let start = quoted.start.max(range.start);
                let end = quoted.end.min(range.end);
                (start < end).then(|| start - range.start..end - range.start)
            })
            .collect::<Vec<_>>();
        crate::filesystem::shell_pattern(&word.text[range], &quoted)
    };
    if exact {
        return known(0..word.text.len());
    }
    let mut gaps = word
        .gaps
        .iter()
        .map(|gap| gap.range.clone())
        .collect::<Vec<_>>();
    if gaps.is_empty() {
        gaps.push(0..word.text.len());
    }
    let mut pattern = String::new();
    let mut cursor = 0;
    for gap in &gaps {
        pattern.push_str(&known(cursor..gap.start.max(cursor)));
        pattern.push('*');
        cursor = cursor.max(gap.end);
    }
    pattern.push_str(&known(cursor..word.text.len()));
    pattern
}

fn parameter_text(
    plain: &Option<String>,
    computed: Option<parameters::Value>,
    spelling: &str,
    quoted: bool,
    expansion: &ExpansionContext<'_>,
    out: &mut Expanded,
    splitting: &mut bool,
) -> bool {
    let start = out.word.text.len();
    let mut inherited_pattern = false;
    let variables = expansion.variables;
    let host = expansion.host;
    if let Some(value) = computed {
        if !value.gaps.is_empty() {
            out.unknown_splitting |= !quoted;
        }
        if value.lexical {
            out.lexical_ranges.push(start..start + value.text.len());
        }
        out.word.text.push_str(&value.text);
        out.word
            .gaps
            .extend(value.gaps.into_iter().map(|gap| crate::record::Gap {
                range: start + gap.range.start..start + gap.range.end,
                nonempty: gap.nonempty,
            }));
        *splitting |= !quoted;
        out.word.globs |= !quoted && value.text.contains(['*', '?', '[']);
    } else if let Some((value, assigned)) = plain.as_deref().and_then(|name| {
        out.assignments
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| (value.as_str(), true))
            .or_else(|| {
                expansion
                    .get(name)
                    .map(String::as_str)
                    .or_else(|| {
                        (name == "HOME"
                            && !expansion.unset(name)
                            && !expansion.facts.inherited.contains(name))
                        .then_some(host.home)
                    })
                    .or_else(|| (name == "PWD" && !expansion.unset(name)).then_some(expansion.cwd))
                    .map(|value| (value, false))
            })
    }) {
        let name = plain.as_deref().unwrap_or_default();
        if let Some(ranges) = expansion.pattern_variables.get(name) {
            // A captured pathname expansion keeps its original
            // pattern domain through a later quoted reference.
            out.word.quoted_ranges.extend(
                ranges
                    .iter()
                    .map(|range| start + range.start..start + range.end),
            );
            inherited_pattern = true;
        }
        let cwd = name == "PWD" && !variables.contains_key("PWD");
        if cwd {
            out.word.pwd = true;
            out.word.cwd_ranges.push(start..start + value.len());
        }
        if expansion.runtime_variables.contains(name) {
            out.word.runtime_unknown = true;
            out.unknown_splitting |= !quoted;
            // Lexical representatives describe unknown output;
            // their whitespace is not a runtime field boundary.
            out.lexical_ranges.push(start..start + value.len());
        }
        out.word.text.push_str(value);
        let gaps = if assigned {
            Vec::new()
        } else if cwd && expansion.facts.cwd_unknown {
            vec![crate::record::Gap {
                range: 0..value.len(),
                nonempty: true,
            }]
        } else {
            expansion.gaps(name, value)
        };
        out.word
            .gaps
            .extend(gaps.into_iter().map(|gap| crate::record::Gap {
                range: start + gap.range.start..start + gap.range.end,
                nonempty: gap.nonempty,
            }));
        *splitting |= !quoted;
        // Keep Bash pathname expansion even when Zsh leaves the binding literal.
        out.word.globs |= !quoted && value.contains(['*', '?', '[']);
    } else if let Some(name) = plain.as_deref().filter(|name| expansion.unset(name)) {
        if expansion.facts.hypotheses.contains(name) {
            out.word.gaps.push(crate::record::Gap {
                range: start..start,
                nonempty: false,
            });
        }
        *splitting |= !quoted;
    } else {
        out.word.text.push_str(spelling);
        out.word.gaps.push(crate::record::Gap {
            range: start..out.word.text.len(),
            nonempty: false,
        });
        out.word.expands = true;
        out.unknown_splitting |= !quoted;
    }
    inherited_pattern
}
