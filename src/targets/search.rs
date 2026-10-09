use super::*;

pub(super) fn infer_search(
    program: &str,
    args: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    let (mut operands, globs, hidden, names, recursive) =
        search_arguments(program, args, cwd, host, effects);
    let implicit = operands.is_empty() && (program != "grep" || hidden);
    if implicit {
        operands.push(Word::literal(cwd.to_owned()));
    }
    effects.hidden_listing = names && hidden;
    let hidden_walk = recursive || matches!(program, "rg" | "ag") && hidden;
    effects.hidden_content = hidden && !names && !hidden_walk;
    for root in operands {
        let mut target = Target::from_word(
            &root,
            cwd,
            host,
            if names && !hidden_walk {
                Effect::List
            } else {
                Effect::Read
            },
            if hidden_walk && !names {
                Walk::Hidden
            } else if program != "grep" || hidden {
                Walk::Visible
            } else {
                Walk::None
            },
        );
        target.search = implicit && !args.iter().any(|arg| arg == "--help" || arg == "-h");
        effects.targets.push(target);
        if !names {
            for glob in &globs {
                if !glob.starts_with('!') {
                    let mut target = Target::new(
                        crate::filesystem::absolute_input(
                            &format!(
                                "{}/{}",
                                if root.globs || root.shell_matches {
                                    root.text.clone()
                                } else {
                                    crate::filesystem::literal_glob_root(&root.text)
                                },
                                glob.rsplit('/').next().unwrap_or(glob)
                            ),
                            cwd,
                            host.home,
                        ),
                        Effect::Read,
                        Walk::None,
                        Via::Operand,
                    );
                    target.glob = true;
                    target.glob_hidden = program == "grep" || hidden;
                    effects.targets.push(target);
                }
            }
        }
    }
}

fn search_arguments(
    program: &str,
    args: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) -> (Vec<Word>, Vec<String>, bool, bool, bool) {
    let mut operands = Vec::new();
    let mut globs = Vec::new();
    let mut explicit = false;
    let mut names = false;
    let mut hidden = false;
    let mut recursive = false;
    let mut no_hidden = false;
    let mut unrestricted = 0;
    let mut options = true;
    let mut position = 0;
    let mut fixed = false;
    let mut patterns = Vec::new();
    while position < args.len() {
        let arg = &args[position];
        if options && arg == "--" {
            options = false;
            position += 1;
            continue;
        }
        let mut value_option = None;
        if options && arg.starts_with("--") {
            let (key, value) = arg[2..]
                .split_once('=')
                .map_or((&arg[2..], None), |(k, v)| {
                    (k, Some(arg.with_text(v.to_owned())))
                });
            if key == "include" && program == "rg" {
                effects.include_advice = true;
            }
            fixed |= key == "fixed-strings";
            names |= key == "files";
            long_visibility(
                program,
                key,
                &mut hidden,
                &mut no_hidden,
                &mut unrestricted,
                &mut recursive,
            );
            if (if program=="rg" {"regexp file glob iglob type type-not encoding replace color colors sort sortr max-depth max-filesize pre pre-glob engine threads max-columns type-add type-clear path-separator context-separator field-context-separator field-match-separator after-context before-context context max-count ignore-file dfa-size-limit regex-size-limit hyperlink-format"} else {"regexp file include exclude exclude-dir exclude-from label context after-context before-context max-count binary-files devices directories"}).split_whitespace().any(|option|option==key)
            {
                value_option = Some((key.to_owned(), value));
            }
        } else if options && arg.starts_with('-') && arg.len() > 1 {
            let (value, is_fixed, next_hidden, next_recursive, next_unrestricted) = short_flags(
                program,
                arg,
                no_hidden,
                hidden,
                recursive,
                unrestricted,
                effects,
            );
            value_option = value.map(|(offset, ch)| short_value(program, arg, offset, ch));
            fixed |= is_fixed;
            hidden = next_hidden;
            recursive = next_recursive;
            unrestricted = next_unrestricted;
        } else {
            operands.push(arg.clone());
        }
        if let Some((key, inline)) = value_option {
            let value = inline.or_else(|| {
                position += 1;
                args.get(position).cloned()
            });
            if let Some(value) = value {
                match key.as_str() {
                    "d" | "directories" if program == "grep" && value == "recurse" => {
                        hidden = true;
                        recursive = true;
                    }
                    "e" | "regexp" => {
                        explicit = true;
                        patterns.push(value.text);
                    }
                    "f" | "file" | "ignore-file" | "exclude-from" => {
                        explicit |= matches!(key.as_str(), "f" | "file");
                        effects.targets.push(Target::from_word(
                            &value,
                            cwd,
                            host,
                            Effect::Read,
                            Walk::None,
                        ));
                    }
                    "g" | "glob" | "iglob" | "include" => globs.push(value.text),
                    _ => {}
                }
            }
        }
        position += 1;
    }
    if !explicit && !names && !operands.is_empty() {
        patterns.push(operands.remove(0).text);
    }
    effects.independent_arguments = independent_operands(args, &operands);
    effects.bre_advice = bre_advice(program, fixed, &patterns);
    (operands, globs, hidden, names, recursive)
}

fn long_visibility(
    program: &str,
    key: &str,
    hidden: &mut bool,
    no_hidden: &mut bool,
    unrestricted: &mut usize,
    recursive: &mut bool,
) {
    match key {
        "hidden" => {
            *hidden = true;
            *no_hidden = false;
        }
        "no-hidden" => {
            *hidden = false;
            *no_hidden = true;
        }
        "unrestricted" => {
            *unrestricted += 1;
            *hidden |= (program == "ag" || *unrestricted >= 2) && !*no_hidden;
        }
        "recursive" => {
            *recursive = program == "grep";
            *hidden |= *recursive;
        }
        _ => {}
    }
}

fn short_value(program: &str, arg: &Word, offset: usize, ch: char) -> (String, Option<Word>) {
    let tail = &arg[offset + ch.len_utf8()..];
    (
        ch.to_string(),
        if tail.is_empty() {
            None
        } else {
            Some(
                arg.with_text(
                    if program == "rg" {
                        tail.trim_start_matches('=')
                    } else {
                        tail
                    }
                    .to_owned(),
                ),
            )
        },
    )
}

fn bre_advice(program: &str, fixed: bool, patterns: &[String]) -> bool {
    // Regex advice applies to patterns, never to path operands or fixed strings.
    program == "rg"
        && !fixed
        && patterns.iter().any(|pattern| {
            pattern
                .as_bytes()
                .windows(2)
                .enumerate()
                .any(|(index, bytes)| {
                    bytes == b"\\|" && (index == 0 || pattern.as_bytes()[index - 1] != b'\\')
                })
        })
}

fn short_flags(
    program: &str,
    arg: &Word,
    no_hidden: bool,
    mut hidden: bool,
    mut recursive: bool,
    mut unrestricted: usize,
    effects: &mut Effects,
) -> (Option<(usize, char)>, bool, bool, bool, usize) {
    let mut fixed = false;
    for (offset, ch) in arg.char_indices().skip(1) {
        if program == "rg" {
            fixed |= ch == 'F';
            if ch == 'u' {
                unrestricted += 1;
                hidden |= unrestricted >= 2 && !no_hidden;
            }
            if ch == '.' {
                hidden = true;
            }
            if ch == 'r' {
                effects.replace_advice = true;
            }
        } else if program == "grep" && (ch == 'r' || ch == 'R') || program == "ag" && ch == 'u' {
            hidden = true;
            recursive |= program == "grep";
        }
        if (if program == "rg" {
            "efgtTEABCmMjrd"
        } else {
            "efABCmdD"
        })
        .contains(ch)
        {
            return (Some((offset, ch)), fixed, hidden, recursive, unrestricted);
        }
    }
    (None, fixed, hidden, recursive, unrestricted)
}
