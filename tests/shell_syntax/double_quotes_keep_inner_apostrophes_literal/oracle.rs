use super::*;

#[derive(Default)]
struct Oracle {
    token_length_mismatches: Vec<Value>,
    substitution_end_limits: Vec<Value>,
    disagreements: Vec<Value>,
    span_disagreements: Vec<Value>,
    word_refusals: Vec<Value>,
    nested_refusals: Vec<Value>,
    compared: usize,
    words: usize,
    nested_scripts: usize,
    compared_inputs: BTreeSet<String>,
}

impl Oracle {
    fn compare_pieces(
        &mut self,
        raw: &str,
        pieces: &[WordPieceWithSource],
        parent: Quote,
        lexical: &Lexed<'_>,
        base: usize,
        id: &str,
    ) {
        let mut covered = 0;
        for p in pieces {
            if p.end_index <= covered {
                continue;
            }
            let mode = match &p.piece {
                WordPiece::SingleQuotedText(_) => Quote::Single,
                WordPiece::AnsiCQuotedText(_) => Quote::AnsiC,
                WordPiece::DoubleQuotedSequence(_) => Quote::Double,
                WordPiece::GettextDoubleQuotedSequence(_) => Quote::Gettext,
                _ => parent,
            };
            let range = p.start_index.max(covered)..p.end_index;
            assert!(
                raw.get(range.clone()).is_some(),
                "Brush piece offsets: {id}"
            );
            let opaque = matches!(
                &p.piece,
                WordPiece::CommandSubstitution(_)
                    | WordPiece::BackquotedCommandSubstitution(_)
                    | WordPiece::ParameterExpansion(_)
                    | WordPiece::ArithmeticExpression(_)
            );
            let sequence = matches!(
                &p.piece,
                WordPiece::DoubleQuotedSequence(_) | WordPiece::GettextDoubleQuotedSequence(_)
            );
            let mismatched_end = if matches!(
                p.piece,
                WordPiece::CommandSubstitution(_) | WordPiece::BackquotedCommandSubstitution(_)
            ) {
                lexical
                    .substitution_body(base + p.start_index)
                    .filter(|body| body.end + 1 != base + p.end_index)
            } else {
                None
            };
            if let Some(body) = &mismatched_end {
                self.substitution_end_limits.push(json!({"id":id,"word":raw,"brush_span":[base+p.start_index,base+p.end_index],"lexer_body":[body.start,body.end],"disposition":"Brush word substitution ends before the lexer region; production forwards the original lexical body (ruling 30)"}));
                covered = body.end + 1 - base;
            }
            for i in range.clone() {
                if mismatched_end.is_some() && i + 1 == range.end {
                    continue;
                }
                if (opaque || sequence) && i != range.start && i + 1 != range.end {
                    continue;
                }
                let context = lexical.context(base + i);
                self.compared += 1;
                self.compared_inputs.insert(id.to_owned());
                let escaped = matches!(&p.piece, WordPiece::EscapeSequence(_));
                if context.quote != mode || (escaped && !context.escaped) {
                    self.disagreements.push(json!({"id":id,"token":raw,"byte":base+i,"brush_quote":format!("{mode:?}"),"lexer":format!("{context:?}"),"piece":format!("{:?}",p.piece)}));
                }
            }
            match &p.piece {
                WordPiece::DoubleQuotedSequence(inner)
                | WordPiece::GettextDoubleQuotedSequence(inner) => {
                    self.compare_pieces(raw, inner, mode, lexical, base, id)
                }
                WordPiece::CommandSubstitution(_) | WordPiece::BackquotedCommandSubstitution(_) => {
                    let width = if matches!(&p.piece, WordPiece::BackquotedCommandSubstitution(_)) {
                        1
                    } else {
                        2
                    };
                    let body = &raw[range.start + width..range.end - 1];
                    let nested_base = base + range.start + width;
                    self.nested_scripts += 1;
                    let tokens = match brush_parser::uncached_tokenize_str(
                        body,
                        &brush_parser::TokenizerOptions::default(),
                    ) {
                        Ok(tokens) => tokens,
                        Err(error) => {
                            self.nested_refusals
                                .push(json!({"id":id,"body":body,"error":error.to_string()}));
                            continue;
                        }
                    };
                    let program = brush_parser::Parser::builder()
                        .build(std::io::Cursor::new(body.as_bytes()))
                        .parse_program();
                    self.compare_tokens(
                        body,
                        tokens,
                        program.ok().as_ref(),
                        lexical,
                        nested_base,
                        id,
                    );
                }
                _ => {}
            }
        }
    }

    fn compare_tokens(
        &mut self,
        source: &str,
        tokens: Vec<brush_parser::Token>,
        program: Option<&brush_parser::ast::Program>,
        lexical: &Lexed<'_>,
        base: usize,
        id: &str,
    ) {
        let offsets: Vec<_> = source
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(source.len()))
            .collect();
        let heredocs = program
            .map(|p| brush_heredocs(p, &offsets, source))
            .unwrap_or_default();
        for token in tokens {
            let brush_parser::Token::Word(raw, span) = token else {
                continue;
            };
            let local = offsets[span.start.index];
            let mut raw = raw.trim_start();
            let mut local = local + source[local..].len() - source[local..].trim_start().len();
            while let Some(tail) = source[local..].strip_prefix("\\\n") {
                local += 2;
                local += tail.len() - tail.trim_start().len();
            }
            // Brush emits a synthetic end-tag token after consuming its source bytes.
            // The independent AST body boundary and exact tag bytes bind its real location.
            if let Some(tag) = heredocs.iter().find_map(|(span, _)| {
                let tag = span.end + source[span.end..].len()
                    - source[span.end..].trim_start_matches('\t').len();
                let after = tag + raw.len();
                ((local == after
                    || (local == after + 1 && source.as_bytes().get(after) == Some(&b'\n')))
                    && source.get(tag..after) == Some(raw))
                .then_some(tag)
            }) {
                self.span_disagreements.push(json!({"id":id,"token":raw,"reported_byte":base+local,"actual_byte":base+tag,"disposition":"Brush synthetic heredoc end-tag span; mapped by independent AST body and exact original tag bytes"}));
                local = tag;
                for byte in local..local + raw.len() {
                    let context = lexical.context(base + byte);
                    if !context.heredoc_delimiter || context.active() {
                        self.disagreements.push(json!({"id":id,"byte":base+byte,"brush_end_tag":raw,"lexer":format!("{context:?}")}));
                    }
                }
            }
            let actual_end = offsets[span.end.index];
            if let Some(actual) = source.get(local..actual_end)
                && raw.len() != actual_end - local
            {
                self.token_length_mismatches.push(json!({"id":id,"byte":base+local,"brush_token":raw,"original_span":actual,"brush_length":raw.len(),"original_length":actual_end-local}));
                raw = actual;
            }
            for byte in local..(local + raw.len()).min(source.len()) {
                let context = lexical.context(base + byte);
                if context.comment
                    && context.command_depth == lexical.context(base + local).command_depth
                    && context.backtick_depth == lexical.context(base + local).backtick_depth
                {
                    self.disagreements.push(json!({"id":id,"byte":base+byte,"dimension":"word_comment","token":raw,"lexer":format!("{context:?}")}));
                }
            }
            let heredoc = heredocs.iter().find(|(span, _)| span.contains(&local));
            if let Some((span, expands)) = heredoc {
                let parent = lexical.context(base + span.start);
                for byte in span.clone() {
                    let context = lexical.context(base + byte);
                    let nested = *expands
                        && (context.command_depth > parent.command_depth
                            || context.parameter_depth > parent.parameter_depth
                            || context.backtick_depth > parent.backtick_depth
                            || context.arithmetic_depth > parent.arithmetic_depth);
                    if !nested && context.heredoc != Some(!expands) {
                        self.disagreements.push(json!({"id":id,"byte":base+byte,"brush_heredoc_expands":expands,"lexer":format!("{context:?}")}));
                    }
                }
                if !expands {
                    continue;
                }
            }
            let parsed = if heredoc.is_some() {
                word::parse_heredoc(raw, &brush_parser::ParserOptions::default())
            } else {
                word::parse(raw, &brush_parser::ParserOptions::default())
            };
            let pieces = match parsed {
                Ok(pieces) => pieces,
                Err(error) => {
                    self.word_refusals.push(json!({"id":id,"token":raw,"program_parsed":program.is_some(),"error":error.to_string()}));
                    continue;
                }
            };
            self.words += 1;
            self.compare_pieces(raw, &pieces, Quote::Unquoted, lexical, base + local, id);
        }
    }
}

fn brush_heredocs(
    program: &brush_parser::ast::Program,
    offsets: &[usize],
    source: &str,
) -> Vec<(std::ops::Range<usize>, bool)> {
    let mut result = Vec::new();
    for c in &program.complete_commands {
        list(c, &mut result);
    }
    result
        .into_iter()
        .map(|(span, expands, value, remove_tabs)| {
            let start = offsets[span.start.index];
            let mut end = start;
            // Brush's doc span includes the end tag; its value owns the body boundary.
            for expected in value.split_inclusive('\n') {
                let next = source[end..]
                    .find('\n')
                    .map_or(source.len(), |n| end + n + 1);
                let actual = &source[end..next];
                assert_eq!(
                    if remove_tabs {
                        actual.trim_start_matches('\t')
                    } else {
                        actual
                    },
                    expected
                );
                end = next;
            }
            (start..end, expands)
        })
        .collect()
}

use brush_parser::ast::*;
fn redirect(r: &IoRedirect, out: &mut Vec<(SourceSpan, bool, String, bool)>) {
    match r {
        IoRedirect::HereDocument(_, doc) => {
            if let Some(span) = &doc.doc.loc {
                out.push((
                    span.clone(),
                    doc.requires_expansion,
                    doc.doc.value.clone(),
                    doc.remove_tabs,
                ));
            }
        }
        IoRedirect::File(_, _, IoFileRedirectTarget::ProcessSubstitution(_, group)) => {
            list(&group.list, out)
        }
        _ => {}
    }
}
fn list(group: &CompoundList, out: &mut Vec<(SourceSpan, bool, String, bool)>) {
    for item in &group.0 {
        for (_, pipeline) in &item.0 {
            for c in &pipeline.seq {
                command(c, out);
            }
        }
    }
}
fn compound(c: &CompoundCommand, out: &mut Vec<(SourceSpan, bool, String, bool)>) {
    match c {
        CompoundCommand::BraceGroup(g) => list(&g.list, out),
        CompoundCommand::Subshell(g) => list(&g.list, out),
        CompoundCommand::ForClause(g) => list(&g.body.list, out),
        CompoundCommand::ArithmeticForClause(g) => list(&g.body.list, out),
        CompoundCommand::IfClause(g) => {
            list(&g.condition, out);
            list(&g.then, out);
            if let Some(branches) = &g.elses {
                for b in branches {
                    if let Some(c) = &b.condition {
                        list(c, out);
                    }
                    list(&b.body, out);
                }
            }
        }
        CompoundCommand::WhileClause(g) | CompoundCommand::UntilClause(g) => {
            list(&g.0, out);
            list(&g.1.list, out);
        }
        CompoundCommand::CaseClause(g) => {
            for c in &g.cases {
                if let Some(body) = &c.cmd {
                    list(body, out);
                }
            }
        }
        CompoundCommand::Coprocess(g) => command(&g.body, out),
        CompoundCommand::Arithmetic(_) => {}
    }
}
fn redirects(rs: Option<&RedirectList>, out: &mut Vec<(SourceSpan, bool, String, bool)>) {
    if let Some(rs) = rs {
        for r in &rs.0 {
            redirect(r, out);
        }
    }
}
fn command(c: &Command, out: &mut Vec<(SourceSpan, bool, String, bool)>) {
    match c {
        Command::Simple(c) => {
            let items = c
                .prefix
                .iter()
                .flat_map(|p| &p.0)
                .chain(c.suffix.iter().flat_map(|p| &p.0));
            for item in items {
                match item {
                    CommandPrefixOrSuffixItem::IoRedirect(r) => redirect(r, out),
                    CommandPrefixOrSuffixItem::ProcessSubstitution(_, g) => list(&g.list, out),
                    _ => {}
                }
            }
        }
        Command::Compound(c, r) => {
            compound(c, out);
            redirects(r.as_ref(), out);
        }
        Command::Function(c) => {
            compound(&c.body.0, out);
            redirects(c.body.1.as_ref(), out);
        }
        Command::ExtendedTest(_, r) => redirects(r.as_ref(), out),
    }
}

#[cfg(test)]
#[path = "oracle/tests.rs"]
mod tests;
