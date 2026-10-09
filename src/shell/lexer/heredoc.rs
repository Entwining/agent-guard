use super::*;

impl Lexed<'_> {
    pub(super) fn document_open(
        &mut self,
        cursor: &mut usize,
        context: Context,
        depth: usize,
        documents: &mut VecDeque<(String, bool, bool)>,
    ) -> Result<(), LexError> {
        let tail = &self.source[*cursor..];
        let opening = *cursor;
        let strip_tabs = tail.starts_with("<<-");
        *cursor += 2 + usize::from(strip_tabs);
        self.mark(opening..*cursor, context);
        while self
            .source
            .as_bytes()
            .get(*cursor)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
        {
            self.mark(*cursor..*cursor + 1, context);
            *cursor += 1;
        }
        let delimiter_start = *cursor;
        let (delimiter, quoted) = self.delimiter(
            cursor,
            Context {
                heredoc_delimiter: true,
                ..context
            },
            depth,
        )?;
        if *cursor > delimiter_start {
            documents.push_back((delimiter, quoted, strip_tabs));
        }
        Ok(())
    }
    pub(super) fn documents(
        &mut self,
        cursor: &mut usize,
        context: Context,
        documents: &mut VecDeque<(String, bool, bool)>,
    ) -> Result<(), LexError> {
        self.mark(*cursor..*cursor + 1, context);
        *cursor += 1;
        while let Some((delimiter, quoted, strip_tabs)) = documents.pop_front() {
            let body_start = *cursor;
            let mut line = *cursor;
            let mut found = None;
            while line <= self.source.len() {
                let right = self.source[line..]
                    .find('\n')
                    .map_or(self.source.len(), |n| line + n);
                let text = &self.source[line..right];
                if (if strip_tabs {
                    text.trim_start_matches('\t')
                } else {
                    text
                }) == delimiter
                {
                    found = Some((line, right));
                    break;
                }
                if right == self.source.len() {
                    break;
                }
                line = right + 1;
            }
            let body_end = found.map_or(self.source.len(), |(left, _)| left);
            self.heredocs
                .push((body_start..body_end, quoted, strip_tabs));
            let body_context = Context {
                heredoc: Some(quoted),
                ..context
            };
            self.mark(body_start..body_end, body_context);
            if !quoted {
                let body = &self.source[body_start..body_end];
                let nested = Lexed::with_context(body, body_context)?;
                self.context[body_start..body_end].copy_from_slice(&nested.context);
                self.heredocs
                    .extend(nested.heredocs.into_iter().map(|(range, quoted, strip)| {
                        (
                            range.start + body_start..range.end + body_start,
                            quoted,
                            strip,
                        )
                    }));
            }
            *cursor = found.map_or(self.source.len(), |(_, right)| {
                right + usize::from(right < self.source.len())
            });
            if let Some((left, _)) = found {
                self.mark(
                    left..*cursor,
                    Context {
                        heredoc_delimiter: true,
                        ..context
                    },
                );
            }
        }
        Ok(())
    }
    fn delimiter(
        &mut self,
        cursor: &mut usize,
        context: Context,
        depth: usize,
    ) -> Result<(String, bool), LexError> {
        let mut delimiter = String::new();
        let mut quoted = false;
        while *cursor < self.source.len() {
            let byte = self.source.as_bytes()[*cursor];
            if shell_blank(byte) || b";|&<>()".contains(&byte) {
                break;
            }
            let quote = initial_quote(&self.source[*cursor..]);
            if quote != Quote::Unquoted {
                quoted = true;
                let start = *cursor;
                self.quotation(cursor, context, quote, depth)?;
                let dollar = matches!(quote, Quote::AnsiC | Quote::Gettext);
                let body = &self.source[start + 1 + usize::from(dollar)..*cursor - 1];
                if quote == Quote::AnsiC {
                    delimiter.push_str(&crate::shell::words::ansi(body));
                } else if matches!(quote, Quote::Double | Quote::Gettext) {
                    let mut chars = body.chars().peekable();
                    while let Some(ch) = chars.next() {
                        if ch == '\\'
                            && chars
                                .peek()
                                .is_some_and(|next| matches!(next, '\\' | '"' | '$' | '`' | '\n'))
                        {
                            if let Some(next) = chars.next()
                                && next != '\n'
                            {
                                delimiter.push(next);
                            }
                        } else {
                            delimiter.push(ch);
                        }
                    }
                } else {
                    delimiter.push_str(body);
                }
            } else if byte == b'\\' {
                quoted = true;
                let start = *cursor;
                self.escape(cursor, context);
                let body = &self.source[start + 1..*cursor];
                if body != "\n" {
                    delimiter.push_str(body);
                }
            } else {
                let next = self.next(*cursor);
                delimiter.push_str(&self.source[*cursor..next]);
                self.mark(*cursor..next, context);
                *cursor = next;
            }
        }
        Ok((delimiter, quoted))
    }
}
