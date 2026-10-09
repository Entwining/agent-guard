use super::*;

pub(super) fn redirect_list(
    source: &Source<'_>,
    list: Option<&RedirectList>,
    output: &mut Vec<Statement>,
) -> Result<(), CheckError> {
    if let Some(list) = list {
        let mut redirects = Vec::new();
        for value in &list.0 {
            redirect(source, value, &mut redirects, output)?;
        }
        output.push(Statement::Command {
            assignments: Vec::new(),
            argv: Vec::new(),
            redirects,
            pipeline: None,
        });
    }
    Ok(())
}

pub(super) fn redirect(
    source: &Source<'_>,
    value: &IoRedirect,
    redirects: &mut Vec<Redirect>,
    output: &mut Vec<Statement>,
) -> Result<(), CheckError> {
    match value {
        IoRedirect::File(fd, kind, IoFileRedirectTarget::Fd(target)) => {
            let input = matches!(kind, IoFileRedirectKind::DuplicateInput);
            redirects.push(Redirect {
                fd: fd.unwrap_or(if input { 0 } else { 1 }),
                duplicate: true,
                target: Word {
                    raw: target.to_string(),
                    syntax: crate::shell::WordSyntax::Literal,
                    expansions: Vec::new(),
                },
                direction: if input {
                    crate::record::Direction::In
                } else {
                    crate::record::Direction::Out
                },
            });
        }
        IoRedirect::File(
            fd,
            kind,
            IoFileRedirectTarget::Filename(target) | IoFileRedirectTarget::Duplicate(target),
        ) => {
            redirects.push(Redirect {
                fd: fd.unwrap_or(
                    if matches!(
                        kind,
                        IoFileRedirectKind::Read
                            | IoFileRedirectKind::ReadAndWrite
                            | IoFileRedirectKind::DuplicateInput
                    ) {
                        0
                    } else {
                        1
                    },
                ),
                duplicate: matches!(
                    kind,
                    IoFileRedirectKind::DuplicateInput | IoFileRedirectKind::DuplicateOutput
                ),
                target: word(source, target)?,
                direction: if matches!(
                    kind,
                    IoFileRedirectKind::Read
                        | IoFileRedirectKind::ReadAndWrite
                        | IoFileRedirectKind::DuplicateInput
                ) {
                    crate::record::Direction::In
                } else {
                    crate::record::Direction::Out
                },
            });
        }
        IoRedirect::File(fd, kind, IoFileRedirectTarget::ProcessSubstitution(process, group)) => {
            process_redirect(source, fd, kind, process, group, redirects, output)?
        }
        IoRedirect::HereDocument(fd, doc) => heredoc(source, fd, doc, redirects, output)?,
        IoRedirect::HereString(fd, value) => {
            let body = word(source, value)?;
            output.push(Statement::Expansion(body.clone()));
            redirects.push(Redirect {
                fd: fd.unwrap_or(0),
                duplicate: false,
                target: body,
                direction: crate::record::Direction::Herestring,
            });
        }
        IoRedirect::OutputAndError(value, _) => {
            redirects.push(Redirect {
                fd: 1,
                duplicate: false,
                target: word(source, value)?,
                direction: crate::record::Direction::Out,
            });
            redirects.push(Redirect {
                fd: 2,
                duplicate: true,
                target: Word {
                    raw: "1".into(),
                    syntax: crate::shell::WordSyntax::Literal,
                    expansions: Vec::new(),
                },
                direction: crate::record::Direction::Out,
            });
        }
    }
    Ok(())
}

fn heredoc(
    source: &Source<'_>,
    fd: &Option<i32>,
    doc: &IoHereDocument,
    redirects: &mut Vec<Redirect>,
    output: &mut Vec<Statement>,
) -> Result<(), CheckError> {
    let Some(span) = &doc.doc.loc else {
        output.push(Statement::UnsupportedSyntax);
        return Ok(());
    };
    let span = source.range(span)?;
    let Some((range, quoted, strip_tabs)) = source.lexical.heredoc_body(span.start) else {
        output.push(Statement::UnsupportedSyntax);
        return Ok(());
    };
    if range.end > span.end || *quoted == doc.requires_expansion || *strip_tabs != doc.remove_tabs {
        output.push(Statement::UnsupportedSyntax);
    }
    let raw = source.text.get(range.clone()).ok_or(CheckError {
        kind: CheckErrorKind::GuardFault,
    })?;
    let raw = if *strip_tabs {
        raw.split_inclusive('\n')
            .map(|line| line.trim_start_matches('\t'))
            .collect::<String>()
    } else {
        raw.to_owned()
    };
    if raw != doc.doc.value {
        output.push(Statement::UnsupportedSyntax);
    }
    let body = Word {
        raw,
        expansions: source.expansions(range),
        syntax: if !quoted {
            crate::shell::WordSyntax::Heredoc
        } else {
            crate::shell::WordSyntax::Literal
        },
    };
    if !quoted {
        output.push(Statement::Expansion(body.clone()));
    }
    redirects.push(Redirect {
        fd: fd.unwrap_or(0),
        duplicate: false,
        target: body,
        direction: crate::record::Direction::Heredoc,
    });
    Ok(())
}

fn process_redirect(
    source: &Source<'_>,
    fd: &Option<i32>,
    kind: &IoFileRedirectKind,
    process: &ProcessSubstitutionKind,
    group: &SubshellCommand,
    redirects: &mut Vec<Redirect>,
    output: &mut Vec<Statement>,
) -> Result<(), CheckError> {
    let mut records = Vec::new();
    walk_list(source, &group.list, &mut records)?;
    if matches!(process, ProcessSubstitutionKind::Read) {
        redirects.push(Redirect {
            fd: fd.unwrap_or(
                if matches!(
                    kind,
                    IoFileRedirectKind::Read | IoFileRedirectKind::DuplicateInput
                ) {
                    0
                } else {
                    1
                },
            ),
            duplicate: false,
            target: Word {
                raw: "__observed_stream__".into(),
                expansions: Vec::new(),
                syntax: crate::shell::WordSyntax::ProcessInput(Box::new(
                    crate::shell::ProcessInput {
                        body: records,
                        prefix: String::new(),
                    },
                )),
            },
            direction: if matches!(
                kind,
                IoFileRedirectKind::Read | IoFileRedirectKind::DuplicateInput
            ) {
                crate::record::Direction::In
            } else {
                crate::record::Direction::Out
            },
        });
    } else {
        output.push(Statement::Substitution(records));
    }
    Ok(())
}
