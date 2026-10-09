use super::*;

impl Lexed<'_> {
    pub(super) fn mark(&mut self, range: Range<usize>, context: Context) {
        self.context[range].fill(context);
    }
    pub(super) fn next(&self, cursor: usize) -> usize {
        cursor
            + self.source[cursor..]
                .chars()
                .next()
                .map_or(0, char::len_utf8)
    }
    pub(super) fn escape(&mut self, cursor: &mut usize, context: Context) {
        let start = *cursor;
        *cursor += 1;
        if *cursor < self.source.len() {
            *cursor = self.next(*cursor);
        }
        self.mark(
            start..*cursor,
            Context {
                escaped: true,
                ..context
            },
        );
    }
    pub(super) fn quotation(
        &mut self,
        cursor: &mut usize,
        context: Context,
        quote: Quote,
        depth: usize,
    ) -> Result<(), LexError> {
        let start = *cursor;
        let dollar = matches!(quote, Quote::AnsiC | Quote::Gettext);
        let end = if matches!(quote, Quote::Single | Quote::AnsiC) {
            b'\''
        } else {
            b'"'
        };
        *cursor += 1 + usize::from(dollar);
        let inner = Context { quote, ..context };
        self.mark(start..*cursor, inner);
        if matches!(quote, Quote::Single | Quote::AnsiC) {
            while *cursor < self.source.len() {
                let byte = self.source.as_bytes()[*cursor];
                if byte == end {
                    self.mark(*cursor..*cursor + 1, inner);
                    *cursor += 1;
                    return Ok(());
                }
                if byte == b'\\' && quote == Quote::AnsiC {
                    self.escape(cursor, inner);
                } else {
                    let next = self.next(*cursor);
                    self.mark(*cursor..next, inner);
                    *cursor = next;
                }
            }
        } else {
            self.region(cursor, inner, Some(end), depth + 1)?;
            return Ok(());
        }
        Err(LexError::Unterminated { start })
    }
    pub(super) fn region(
        &mut self,
        cursor: &mut usize,
        context: Context,
        end: Option<u8>,
        depth: usize,
    ) -> Result<Option<usize>, LexError> {
        if depth > MAX_NESTING
            || context.command_depth
                + context.parameter_depth
                + context.backtick_depth
                + context.arithmetic_depth
                > MAX_NESTING
        {
            return Err(LexError::Nesting);
        }
        let start = *cursor;
        let mut parens: usize = 0;
        let parameter_pattern = end == Some(b'}')
            && self.source[start..]
                .trim_start_matches(|ch: char| ch.is_ascii_alphanumeric() || ch == '_')
                .starts_with('/');
        let mut word_start = true;
        let mut word_groups = Vec::new();
        let mut braces: usize = 0;
        let mut documents = VecDeque::new();
        while *cursor < self.source.len() {
            let byte = self.source.as_bytes()[*cursor];
            let tail = &self.source[*cursor..];
            if end == Some(byte) && (end == Some(b'}') || parens == 0) && braces == 0 {
                self.mark(*cursor..*cursor + 1, context);
                *cursor += 1;
                return Ok(Some(*cursor));
            }
            if byte == b'\\'
                && ((context.quote == Quote::Unquoted && context.heredoc.is_none())
                    || self.source.as_bytes().get(*cursor + 1).is_some_and(|next| {
                        matches!(next, b'\\' | b'$' | b'`' | b'\n')
                            || (*next == b'"' && context.heredoc.is_none())
                    }))
            {
                let continuation = self.source.as_bytes().get(*cursor + 1) == Some(&b'\n');
                self.escape(cursor, context);
                if !continuation {
                    word_start = false;
                }
                continue;
            }
            if tail.starts_with("$((")
                || (context.unquoted() && context.heredoc.is_none() && tail.starts_with("(("))
            {
                self.arithmetic_group(cursor, context, depth)?;
                word_start = false;
                continue;
            }
            if tail.starts_with("$[") {
                self.arithmetic_bracket(cursor, context, depth)?;
                word_start = false;
                continue;
            }
            if tail.starts_with("$(") || tail.starts_with("${") || byte == b'`' {
                self.substitution(cursor, context, depth)?;
                word_start = false;
                continue;
            }
            if context.heredoc.is_none()
                && (context.quote == Quote::Unquoted || parameter_pattern)
                && self.word_syntax(cursor, context, depth, &mut word_start, &mut documents)?
            {
                continue;
            }
            if byte == b'\n' && !documents.is_empty() {
                self.documents(cursor, context, &mut documents)?;
                continue;
            }
            if context.quote == Quote::Unquoted && context.heredoc.is_none() {
                if byte == b'(' {
                    parens += 1;
                    let process = *cursor > 0
                        && matches!(self.source.as_bytes()[*cursor - 1], b'<' | b'>')
                        && !self.context(*cursor - 1).escaped;
                    word_groups.push(!word_start || process);
                    word_start = true;
                } else if byte == b')' {
                    parens = parens.saturating_sub(1);
                    word_start = !word_groups.pop().unwrap_or(false);
                } else {
                    word_start = shell_blank(byte) || b";|&<>".contains(&byte);
                }
                if end == Some(b'}') && byte == b'{' {
                    braces += 1;
                }
                if byte == b'}' {
                    braces = braces.saturating_sub(1);
                }
            }
            let next = self.next(*cursor);
            self.mark(*cursor..next, context);
            *cursor = next;
        }
        if end.is_some() {
            Err(LexError::Unterminated { start })
        } else {
            Ok(None)
        }
    }
    fn word_syntax(
        &mut self,
        cursor: &mut usize,
        context: Context,
        depth: usize,
        word_start: &mut bool,
        documents: &mut VecDeque<(String, bool, bool)>,
    ) -> Result<bool, LexError> {
        let byte = self.source.as_bytes()[*cursor];
        let tail = &self.source[*cursor..];
        let quote = initial_quote(tail);
        if quote != Quote::Unquoted {
            self.quotation(cursor, context, quote, depth)?;
            *word_start = false;
            return Ok(true);
        }
        if byte == b'#'
            && *word_start
            && (context.command_syntax
                || (context.parameter_depth == 0 && context.arithmetic_depth == 0))
        {
            let right = tail.find('\n').map_or(self.source.len(), |n| *cursor + n);
            self.mark(
                *cursor..right,
                Context {
                    comment: true,
                    ..context
                },
            );
            *cursor = right;
            return Ok(true);
        }
        if tail.starts_with("<<<")
            && (context.parameter_depth == 0 && context.arithmetic_depth == 0)
        {
            self.mark(*cursor..*cursor + 3, context);
            *cursor += 3;
            *word_start = true;
            return Ok(true);
        }
        if tail.starts_with("<<")
            && (context.command_syntax
                || (context.parameter_depth == 0 && context.arithmetic_depth == 0))
        {
            self.document_open(cursor, context, depth, documents)?;
            *word_start = false;
            return Ok(true);
        }
        Ok(false)
    }
}
