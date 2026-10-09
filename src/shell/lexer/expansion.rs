use super::*;

impl Lexed<'_> {
    pub(super) fn arithmetic_group(
        &mut self,
        cursor: &mut usize,
        context: Context,
        depth: usize,
    ) -> Result<(), LexError> {
        let byte = self.source.as_bytes()[*cursor];
        let opening = *cursor;
        let width = if byte == b'$' { 3 } else { 2 };
        self.mark(opening..opening + width, context);
        *cursor += width;
        let inner = Context {
            quote: Quote::Unquoted,
            arithmetic_depth: context.arithmetic_depth + 1,
            command_syntax: false,
            heredoc: None,
            ..context
        };
        self.region(cursor, inner, Some(b')'), depth + 1)?;
        if self.source.as_bytes().get(*cursor) != Some(&b')') {
            return Err(LexError::Unterminated { start: opening });
        }
        if *cursor < self.source.len() {
            self.mark(*cursor - 1..*cursor + 1, context);
            *cursor += 1;
        }
        Ok(())
    }
    pub(super) fn arithmetic_bracket(
        &mut self,
        cursor: &mut usize,
        context: Context,
        depth: usize,
    ) -> Result<(), LexError> {
        let opening = *cursor;
        self.mark(opening..opening + 2, context);
        *cursor += 2;
        let inner = Context {
            quote: Quote::Unquoted,
            arithmetic_depth: context.arithmetic_depth + 1,
            command_syntax: false,
            heredoc: None,
            ..context
        };
        self.region(cursor, inner, Some(b']'), depth + 1)?;
        self.mark(*cursor - 1..*cursor, context);
        Ok(())
    }
    pub(super) fn substitution(
        &mut self,
        cursor: &mut usize,
        context: Context,
        depth: usize,
    ) -> Result<(), LexError> {
        let byte = self.source.as_bytes()[*cursor];
        let tail = &self.source[*cursor..];
        let opening = *cursor;
        let backtick = byte == b'`';
        let parameter = tail.starts_with("${");
        let width = if backtick { 1 } else { 2 };
        self.mark(opening..opening + width, context);
        *cursor += width;
        let inner = Context {
            quote: if parameter {
                context.quote
            } else {
                Quote::Unquoted
            },
            command_depth: context.command_depth + usize::from(!parameter && !backtick),
            parameter_depth: context.parameter_depth + usize::from(parameter),
            command_syntax: !parameter,
            backtick_depth: context.backtick_depth + usize::from(backtick),
            heredoc: None,
            ..context
        };
        self.region(
            cursor,
            inner,
            Some(if backtick {
                b'`'
            } else if parameter {
                b'}'
            } else {
                b')'
            }),
            depth + 1,
        )?;
        self.mark(*cursor - 1..*cursor, context);
        Ok(())
    }
}
