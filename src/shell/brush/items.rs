use super::*;

pub(super) fn item_record(
    source: &Source<'_>,
    item: &CommandPrefixOrSuffixItem,
    argv: &mut Vec<Word>,
    redirects: &mut Vec<Redirect>,
    assignments: &mut Vec<(String, Word)>,
    output: &mut Vec<Statement>,
) -> Result<(), CheckError> {
    match item {
        CommandPrefixOrSuffixItem::Word(value) => argv.push(word(source, value)?),
        CommandPrefixOrSuffixItem::AssignmentWord(assignment, value) => {
            assignment_record(source, assignment, value, argv, assignments, output)?
        }
        CommandPrefixOrSuffixItem::IoRedirect(value) => redirect(source, value, redirects, output)?,
        CommandPrefixOrSuffixItem::ProcessSubstitution(kind, group) => {
            let mut records = Vec::new();
            walk_list(source, &group.list, &mut records)?;
            if matches!(kind, ProcessSubstitutionKind::Write) {
                output.push(Statement::Substitution(records.clone()));
            }
            let mut prefix = String::new();
            let start = source.range(&group.loc)?.start.saturating_sub(1);
            if let Some(previous) = argv.last()
                && source.text[..start].ends_with(&previous.raw)
            {
                prefix = argv.pop().map_or(String::new(), |word| word.raw);
            }
            argv.push(Word {
                raw: "__observed_stream__".into(),
                syntax: if matches!(kind, ProcessSubstitutionKind::Read) {
                    crate::shell::WordSyntax::ProcessInput(Box::new(crate::shell::ProcessInput {
                        body: records,
                        prefix,
                    }))
                } else {
                    crate::shell::WordSyntax::Shell
                },
                expansions: Vec::new(),
            });
        }
    }
    Ok(())
}

fn assignment_record(
    source: &Source<'_>,
    assignment: &Assignment,
    value: &WordAst,
    argv: &mut Vec<Word>,
    assignments: &mut Vec<(String, Word)>,
    output: &mut Vec<Statement>,
) -> Result<(), CheckError> {
    let value = word(source, value)?;
    if let AssignmentName::ArrayElementName(name, _) = &assignment.name {
        let left = name.len();
        let (lexical, error) = crate::shell::lexer::Lexed::parameter_fragment(
            &value.raw,
            crate::shell::lexer::Context::default(),
        );
        if error == Some(crate::shell::lexer::LexError::Nesting) {
            return Err(CheckError {
                kind: CheckErrorKind::ResourceLimit,
            });
        }
        if let Some(right) = lexical.closing(left, b'[', b']') {
            let index = value.raw.get(left + 1..right).ok_or(CheckError {
                kind: CheckErrorKind::GuardFault,
            })?;
            output.push(Statement::Expansion(Word {
                raw: index.to_owned(),
                syntax: crate::shell::WordSyntax::Arithmetic,
                expansions: value.expansions.clone(),
            }));
        } else {
            output.push(Statement::UnsupportedSyntax);
        }
    }
    let declaration = argv
        .first()
        .filter(|word| matches!(word.raw.as_str(), "declare" | "local" | "typeset"));
    if !argv.is_empty()
        && (declaration.is_none() || !matches!(assignment.value, AssignmentValue::Array(_)))
    {
        argv.push(value);
        return Ok(());
    }
    if let AssignmentValue::Array(elements) = &assignment.value {
        let mut values = Vec::new();
        for (index, value) in elements {
            let index = if let Some(index) = index {
                let mut index = word(source, index)?;
                index.syntax = crate::shell::WordSyntax::Arithmetic;
                output.push(Statement::Expansion(index.clone()));
                Some(index)
            } else {
                None
            };
            values.push((index, word(source, value)?));
        }
        output.push(Statement::ArrayAssignment {
            name: assignment.name.to_string(),
            values,
            append: assignment.append,
            declaration: declaration.is_some(),
        });
        if declaration.is_some() {
            argv.push(Word {
                raw: assignment.name.to_string(),
                syntax: crate::shell::WordSyntax::Literal,
                expansions: Vec::new(),
            });
        }
        return Ok(());
    }
    if let Some((name, raw)) = value.raw.split_once('=') {
        let (name, target) = if let AssignmentValue::Scalar(target) = &assignment.value {
            let mut name = assignment.name.to_string();
            if assignment.append {
                name.push('+');
            }
            (name, word(source, target)?)
        } else {
            (
                name.to_owned(),
                Word {
                    raw: raw.to_owned(),
                    syntax: crate::shell::WordSyntax::Shell,
                    expansions: value.expansions.clone(),
                },
            )
        };
        assignments.push((name, target));
    }
    Ok(())
}
