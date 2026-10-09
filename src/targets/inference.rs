use super::*;

pub fn infer(command: &CommandRecord, cwd: &str, host: HostFacts<'_>) -> Effects {
    infer_at(command, cwd, host, 0)
}

pub(super) fn infer_at(
    command: &CommandRecord,
    cwd: &str,
    host: HostFacts<'_>,
    depth: usize,
) -> Effects {
    let mut effects = Effects::default();
    #[cfg(test)]
    {
        effects.owner_visits += 1;
        effects.argv_words += command.argv.len();
    }
    if depth > crate::limits::MAX_NESTING {
        effects.gaps.push(CoverageGap::InspectionBudget);
        return effects;
    }
    infer_redirects(command, cwd, host, &mut effects);
    if command.function {
        if command.argv.iter().any(|word| word.field_count_unknown) {
            effects.gaps.push(CoverageGap::UnsupportedShellSyntax);
        }
        return effects;
    }
    if command.program.is_none() {
        infer_unselected(command, cwd, host, &mut effects);
        return effects;
    }
    let index = command.program.unwrap_or(0);
    let Some(program) = command
        .argv
        .get(index)
        .map(|s| s.rsplit('/').next().unwrap_or(s))
    else {
        return effects;
    };
    let program = match program {
        "egrep" | "fgrep" => "grep",
        name => name,
    };
    let cwd_program = program;
    let program = ["python", "node", "ruby", "perl", "php", "lua"]
        .into_iter()
        .find(|base| {
            program.strip_prefix(base).is_some_and(|suffix| {
                !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit() || c == '.')
            })
        })
        .unwrap_or(program);
    let args = &command.argv[index + 1..];
    let args = label_options(program, args, cwd, host, &mut effects);
    let args = args.as_ref();
    infer_inputs(program, command, cwd, host, &mut effects);
    let hidden_items_read = infer_items(program, command, &mut effects);
    infer_xargs(program, command, cwd, host, &mut effects);
    secrets::infer(program, args, &mut effects);
    let (generic_walk, claimed) =
        programs::infer(program, args, command, cwd, host, &mut effects, depth);
    if let Some(walk) = generic_walk {
        infer_operands(program, args, cwd, host, &mut effects, walk, &claimed);
    }
    effects.hidden_content |= hidden_items_read;
    infer_completion(program, args, command, &mut effects);
    infer_cwd(cwd_program, cwd, &mut effects);
    effects
}

fn infer_redirects(command: &CommandRecord, cwd: &str, host: HostFacts<'_>, effects: &mut Effects) {
    for redirect in &command.redirects {
        if redirect.stream.is_some() {
            continue;
        }
        if matches!(
            redirect.direction,
            Direction::Heredoc | Direction::Herestring
        ) {
            continue;
        }
        let path = &redirect.target;
        let write = redirect.direction == Direction::Out;
        let mut target = Target::new(
            crate::filesystem::absolute_input(path, cwd, host.home),
            if write { Effect::Write } else { Effect::Read },
            Walk::None,
            Via::Redirect,
        );
        target.glob = redirect.globs || redirect.shell_matches;
        target.pattern = redirect.pattern.as_ref().map(|pattern| {
            crate::filesystem::absolute_pattern(
                &crate::filesystem::expand_home(
                    pattern,
                    &crate::filesystem::literal_shell_pattern(host.home),
                    host.user,
                ),
                cwd,
            )
        });
        target.glob_hidden = !redirect.globs && !redirect.shell_matches;
        target.expands = redirect.expands;
        target.runtime_unknown = redirect.runtime_unknown;
        effects.targets.push(target);
    }
}

fn infer_unselected(
    command: &CommandRecord,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    effects.dump = command.wrappers.last().is_some_and(|w| w == "env")
        && !command.wrappers.iter().any(|w| w == "env-S");
    for word in &command.argv {
        if !matches!(
            word.role,
            crate::record::Role::Precommand
                | crate::record::Role::Assign
                | crate::record::Role::Namespace
        ) {
            effects
                .targets
                .push(Target::from_word(word, cwd, host, Effect::Use, Walk::None));
        }
    }
}

fn infer_inputs(
    program: &str,
    command: &CommandRecord,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    let index = command.program.unwrap_or(0);
    if !command.wrappers.iter().any(|wrapper| wrapper == "xargs") {
        for stream in list_file_sources(command) {
            for output in &stream.known {
                for path in output.lines().filter(|line| !line.is_empty()) {
                    effects.targets.push(Target::from_word(
                        &Word::literal(path.into()),
                        cwd,
                        host,
                        if program == "du" {
                            Effect::List
                        } else {
                            Effect::Read
                        },
                        Walk::None,
                    ));
                }
            }
            if stream.unknown {
                effects.gaps.push(CoverageGap::UnresolvedTarget);
            }
        }
    }
    if command.argv[index].contains('/') {
        let mut target =
            Target::from_word(&command.argv[index], cwd, host, Effect::Use, Walk::Visible);
        target.via = Via::Option;
        effects.targets.push(target);
    }
    if command.stdin == crate::record::Stdin::Code {
        effects.inline.extend(
            command
                .redirects
                .iter()
                .filter(|r| matches!(r.direction, Direction::Heredoc | Direction::Herestring))
                .map(|r| r.target.clone()),
        );
    }
}

fn infer_items(program: &str, command: &CommandRecord, effects: &mut Effects) -> bool {
    if let Some(items) = &command.items {
        let effect = match program {
            "echo" | "printf" | "print" | ":" | "true" | "false" | "export" | "set" | "unset"
            | "typeset" | "declare" | "local" | "cd" | "curl" | "wget" | "docker" | "ssh" => {
                Effect::Name
            }
            "rm" | "mv" | "ln" | "stat" | "test" | "[" | "chmod" | "chown" | "chgrp"
            | "chflags" | "touch" | "rmdir" | "mkdir" | "wc" | "file" | "shasum" | "sha1sum"
            | "sha256sum" | "md5" | "md5sum" | "cksum" | "realpath" | "readlink" | "basename"
            | "dirname" => Effect::Meta,
            "ls" | "tree" | "du" | "find" | "fd" => Effect::List,
            "pushd" | "popd" => Effect::Enter,
            "tee" => Effect::Write,
            "ssh-add" => Effect::Use,
            _ => Effect::Read,
        };
        let walk = if items.hidden {
            Walk::Hidden
        } else {
            Walk::Visible
        };
        effects
            .targets
            .push(Target::new(items.root.clone(), effect, walk, Via::Items));
        effect == Effect::Read && walk == Walk::Hidden
    } else {
        false
    }
}

fn infer_xargs(
    program: &str,
    command: &CommandRecord,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    let index = command.program.unwrap_or(0);
    if command.wrappers.iter().any(|w| w == "xargs") {
        effects.consumes_listing = xargs_content_consumer(program);
        let options = &command.argv[..index];
        for (i, word) in options.iter().enumerate() {
            let file = if word == "-a" || word == "--arg-file" {
                options.get(i + 1).cloned()
            } else {
                word.strip_prefix("--arg-file=")
                    .map(|path| word.with_text(path.into()))
            };
            if let Some(file) = file {
                let mut target = Target::from_word(&file, cwd, host, Effect::Read, Walk::None);
                target.via = Via::Option;
                effects.targets.push(target);
            }
        }
    }
}

fn infer_operands(
    program: &str,
    args: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
    walk: Walk,
    claimed: &[usize],
) {
    // Unclaimed glued values reach the generic operand role; program-specific
    // adapters retain ownership of the arguments they already claimed.
    for (index, arg) in args.iter().enumerate() {
        if !claimed.contains(&index)
            && arg.role != Role::Option(OptionRole::Name)
            && arg.as_str() != "__observed_stream__"
            && let Some(value) = if arg.role == Role::Path {
                (!arg.value.is_empty()).then_some(arg.value.as_str())
            } else {
                operand_value(arg)
            }
        {
            effects.targets.push(Target::from_word(
                &arg.with_text(value.into()),
                cwd,
                host,
                if !modelled_program(program)
                    && value.contains("://")
                    && !value
                        .get(..7)
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
                {
                    Effect::Name
                } else {
                    Effect::Read
                },
                walk,
            ));
        }
    }
}

fn infer_completion(program: &str, args: &[Word], command: &CommandRecord, effects: &mut Effects) {
    // A repetition projects each literal, not every possible argument sequence.
    // Positional roles and executable text cannot use those projections as a
    // complete command. Independent operand owners above can check their union;
    // transformations also need a complete resource spelling.
    if !effects.independent_arguments && args.iter().any(|word| word.cardinality_unknown)
        || args
            .iter()
            .any(|word| word.cardinality_unknown && word.expands)
            && effects.targets.iter().any(|target| {
                target.effect != Effect::Name && !matches!(target.via, Via::Redirect | Via::Cwd)
            })
    {
        effects.gaps.push(CoverageGap::UnsupportedShellSyntax);
    }
    effects.dump |= effects.inline.iter().any(|code| printenv_signature(code));
    let display = READERS
        .split_whitespace()
        .chain(["echo", "printf", "print"])
        .any(|name| name == program)
        && !(program != "echo"
            && ["printf", "print"].contains(&program)
            && args.first().is_some_and(|arg| arg == "-v"));
    effects.variable |= display
        && command
            .redirects
            .iter()
            .flat_map(|r| &r.vars)
            .any(|name| secret_name(name));
}

fn infer_cwd(cwd_program: &str, cwd: &str, effects: &mut Effects) {
    // Unknown programs also enter the command cwd; identifying their readable
    // arguments does not model the rest of their behavior.
    let named = effects.targets.iter().any(|target| {
        matches!(target.via, Via::Operand | Via::Cwd | Via::Scan)
            && !matches!(target.effect, Effect::Enter | Effect::Name)
    });
    if !DATA_PROGRAMS
        .split_whitespace()
        .any(|name| name == cwd_program)
        && !["cd", "pushd", "popd"].contains(&cwd_program)
        && (!named || !modelled_program(cwd_program))
    {
        effects.targets.push(Target::new(
            cwd.to_owned(),
            Effect::Enter,
            Walk::None,
            Via::Cwd,
        ));
    }
}
