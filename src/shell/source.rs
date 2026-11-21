use super::*;

impl statements::Evaluator<'_, '_> {
    pub(super) fn parsed_source(&mut self, source: &str) -> Result<Rc<SourceSyntax>, CheckError> {
        crate::check_deadline(self.deadline)?;
        if let Some(parsed) = self.parse_cache.get(source) {
            return Ok(Rc::clone(parsed));
        }
        #[cfg(test)]
        {
            self.output.parse_builds += 1;
        }
        let original = brush::records(source, source)?;
        crate::check_deadline(self.deadline)?;
        let original_records = original.records.map(Rc::new);
        let lexical = match lexer::Lexed::scan(source) {
            Ok(lexical) => Some(lexical),
            Err(lexer::LexError::Nesting) => {
                return Err(CheckError {
                    kind: CheckErrorKind::ResourceLimit,
                });
            }
            Err(lexer::LexError::Unterminated { .. }) => None,
        };
        let detection = lexical
            .as_ref()
            .map(|lexical| divergence::detect_lexed(source, &original.spans, lexical))
            .transpose()?;
        let records = if let Some(detection) = &detection
            && detection.masked != source
        {
            brush::records(source, &detection.masked)?
                .records
                .map(Rc::new)
        } else {
            original_records.clone()
        };
        crate::check_deadline(self.deadline)?;
        let parsed = Rc::new(SourceSyntax {
            original: original_records,
            records,
            detection,
        });
        self.parse_cache
            .insert(source.to_owned(), Rc::clone(&parsed));
        Ok(parsed)
    }

    pub(super) fn source(
        &mut self,
        source: &str,
        scope: &mut statements::Scope,
        depth: usize,
    ) -> Result<crate::record::stream::Output, CheckError> {
        #[cfg(test)]
        {
            self.output.source_entries += 1;
        }
        let Frontend { zsh, .. } = self.frontend;
        scope.zsh = zsh;
        // NUL is ignored even inside a word, rather than splitting its bytes.
        let without_nul = source.contains('\0').then(|| source.replace('\0', ""));
        let source = without_nul.as_deref().unwrap_or(source);
        crate::check_deadline(self.deadline)?;
        if depth > MAX_NESTING {
            return Err(CheckError {
                kind: CheckErrorKind::ResourceLimit,
            });
        }
        let parsed = self.parsed_source(source)?;
        crate::check_deadline(self.deadline)?;
        let source_id = self.output.parse_successes + self.output.parse_failures;
        if parsed.original.is_some() {
            self.output.parse_successes += 1;
        } else {
            self.output.script.parse_failed = true;
            self.output.parse_failures += 1;
        }
        let Some(detection) = &parsed.detection else {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
            return self.runnable_prefix(source, scope, depth);
        };
        if !detection.array_tail_spans.is_empty() {
            self.output.array_tail_regions.push(ArrayTailRegions {
                source: source.to_owned(),
                ranges: detection.array_tail_spans.clone(),
            });
        }
        if detection.divergent {
            self.output.gap(if zsh {
                CoverageGap::ExecutorDivergence
            } else {
                CoverageGap::UnsupportedDialectConstruct
            });
        }
        let output = if let Some(records) = &parsed.records {
            self.run(records, scope, depth, source_id, depth > 0)?
        } else {
            if !detection.divergent {
                self.output.gap(CoverageGap::UnsupportedShellSyntax);
            }
            if parsed.original.is_some() {
                Default::default()
            } else {
                self.runnable_prefix(source, scope, depth)?
            }
        };
        for code in &detection.code {
            self.isolated_source(code, scope, depth + 1)?;
        }
        Ok(output)
    }

    /// Bash runs the complete commands before a syntax error, so the lines
    /// before the first one brush cannot parse are checked as their own source.
    fn runnable_prefix(
        &mut self,
        source: &str,
        scope: &mut statements::Scope,
        depth: usize,
    ) -> Result<crate::record::stream::Output, CheckError> {
        let mut end = source.len();
        while let Some(line) = brush::failure_line(&source[..end]) {
            crate::check_deadline(self.deadline)?;
            end = source[..end]
                .split_inclusive('\n')
                .take(line - 1)
                .map(str::len)
                .sum();
        }
        if end == 0 || end == source.len() {
            return Ok(Default::default());
        }
        self.source(&source[..end], scope, depth)
    }
}
