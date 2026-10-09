use super::*;

pub(super) fn walk_compound(
    source: &Source<'_>,
    command: &CompoundCommand,
    output: &mut Vec<Statement>,
) -> Result<(), CheckError> {
    match command {
        CompoundCommand::BraceGroup(group) => {
            let mut body = Vec::new();
            walk_list(source, &group.list, &mut body)?;
            output.push(Statement::Group(body));
        }
        CompoundCommand::Subshell(group) => {
            let mut body = Vec::new();
            walk_list(source, &group.list, &mut body)?;
            output.push(Statement::Subshell(body));
        }
        CompoundCommand::ForClause(group) => walk_for(source, group, output)?,
        CompoundCommand::ArithmeticForClause(group) => walk_arithmetic_for(source, group, output)?,
        CompoundCommand::IfClause(group) => walk_if(source, group, output)?,
        CompoundCommand::WhileClause(group) | CompoundCommand::UntilClause(group) => {
            let mut body = Vec::new();
            walk_list(source, &group.0, &mut body)?;
            walk_list(source, &group.1.list, &mut body)?;
            output.push(Statement::Loop {
                variable: None,
                header: Vec::new(),
                body,
                empty: false,
            });
        }
        CompoundCommand::CaseClause(group) => walk_case(source, group, output)?,
        CompoundCommand::Coprocess(group) => walk_command(source, &group.body, output, None)?,
        CompoundCommand::Arithmetic(group) => output.push(Statement::Expansion(Word {
            raw: group.expr.value.clone(),
            syntax: crate::shell::WordSyntax::Arithmetic,
            expansions: source.expansions(&source.range(&group.loc)?),
        })),
    }
    Ok(())
}

fn walk_for(
    source: &Source<'_>,
    group: &ForClauseCommand,
    output: &mut Vec<Statement>,
) -> Result<(), CheckError> {
    // brush 0.4 uses None for both an omitted list and an explicit empty list.
    let start = source.range(&group.loc)?.start;
    let end = source.range(&group.body.loc)?.start;
    let explicit_in = source.text[start..end]
        .match_indices("in")
        .any(|(offset, _)| {
            let index = start + offset;
            source.lexical.context(index).word_syntax()
                && index > start
                && crate::shell::lexer::shell_blank(source.text.as_bytes()[index - 1])
                && source
                    .text
                    .as_bytes()
                    .get(index + 2)
                    .is_some_and(|b| crate::shell::lexer::shell_blank(*b) || *b == b';')
        });
    let mut header: Vec<Word> = group
        .values
        .iter()
        .flatten()
        .map(|v| word(source, v))
        .collect::<Result<_, _>>()?;
    if !explicit_in && group.values.is_none() {
        header.push(Word {
            raw: "\"$@\"".into(),
            syntax: crate::shell::WordSyntax::Shell,
            expansions: Vec::new(),
        });
    }
    let mut body = Vec::new();
    walk_list(source, &group.body.list, &mut body)?;
    output.push(Statement::Loop {
        variable: Some(group.variable_name.clone()),
        header,
        body,
        empty: explicit_in && group.values.as_ref().is_none_or(Vec::is_empty),
    });
    Ok(())
}

fn walk_arithmetic_for(
    source: &Source<'_>,
    group: &ArithmeticForClauseCommand,
    output: &mut Vec<Statement>,
) -> Result<(), CheckError> {
    let range = source.range(&group.loc)?;
    let header = original(source, &group.loc)?;
    if let Some(left) = header.find("((").map(|i| i + range.start)
        && let Some(right) = source.lexical.closing(left, b'(', b')')
        && right < range.end
    {
        output.push(Statement::Expansion(Word {
            raw: source.text[left + 2..right - 1].to_owned(),
            syntax: crate::shell::WordSyntax::Arithmetic,
            expansions: source.expansions(&(left + 2..right - 1)),
        }));
    } else {
        output.push(Statement::UnsupportedSyntax);
    }
    let mut body = Vec::new();
    walk_list(source, &group.body.list, &mut body)?;
    output.push(Statement::Loop {
        variable: None,
        header: Vec::new(),
        body,
        empty: false,
    });
    Ok(())
}

fn walk_if(
    source: &Source<'_>,
    group: &IfClauseCommand,
    output: &mut Vec<Statement>,
) -> Result<(), CheckError> {
    let mut condition = Vec::new();
    walk_list(source, &group.condition, &mut condition)?;
    let mut then = Vec::new();
    walk_list(source, &group.then, &mut then)?;
    let mut otherwise = Vec::new();
    if let Some(elses) = &group.elses {
        for branch in elses.iter().rev() {
            let mut body = Vec::new();
            walk_list(source, &branch.body, &mut body)?;
            if let Some(test) = &branch.condition {
                let mut condition = Vec::new();
                walk_list(source, test, &mut condition)?;
                otherwise = vec![Statement::Conditional {
                    condition,
                    then: body,
                    otherwise,
                }];
            } else {
                otherwise = body;
            }
        }
    }
    output.push(Statement::Conditional {
        condition,
        then,
        otherwise,
    });
    Ok(())
}

fn walk_case(
    source: &Source<'_>,
    group: &CaseClauseCommand,
    output: &mut Vec<Statement>,
) -> Result<(), CheckError> {
    let mut words = vec![word(source, &group.value)?];
    let mut branches = Vec::new();
    let mut exhaustive = false;
    for case in &group.cases {
        for pattern in &case.patterns {
            let w = word(source, pattern)?;
            exhaustive |= w.raw == "*";
            words.push(w);
        }
        let mut body = Vec::new();
        if let Some(commands) = &case.cmd {
            walk_list(source, commands, &mut body)?;
        }
        branches.push(body);
    }
    output.push(Statement::Case {
        words,
        branches,
        exhaustive,
    });
    Ok(())
}
