use super::*;
use crate::record::Listed;

/// The effects of `command` when the shell runs it in `cwd`.
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
    infer_wrapper_files(command, cwd, host, &mut effects);
    let cwd = &crate::record::entered(cwd, &command.argv, &command.wrapper_files, host.home);
    if command.function {
        if command.argv.iter().any(|word| word.field_count_unknown) {
            effects.gaps.push(CoverageGap::UnsupportedShellSyntax);
        }
        return effects;
    }
    // A program the shell does not fix proves no owner's roles, and neither
    // does a wrapper word its grammar cannot place.
    let unproven = command.unproven.or(command
        .program
        .filter(|index| command.argv[*index].exact().is_none()));
    if let Some(start) = unproven {
        infer_unproven(&command.argv[start..], cwd, host, &mut effects);
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
    let args = label_options(args);
    let args = args.as_ref();
    infer_inputs(program, command, cwd, host, &mut effects);
    let hidden_items_read = infer_items(program, command, host, &mut effects);
    let consumer = effects.targets.len();
    infer_xargs(program, command, &mut effects);
    secrets::infer(program, args, &mut effects);
    let (generic_walk, claimed) =
        programs::infer(program, args, command, cwd, host, &mut effects, depth);
    if let Some(walk) = generic_walk {
        infer_operands(program, args, cwd, host, &mut effects, walk, &claimed);
    }
    if let Some(start) = effects.unproven {
        infer_unproven(&args[start..], cwd, host, &mut effects);
    }
    match hidden_items_read {
        Some(Some((walks, files))) => {
            let walks = listing::handed(walks, &effects.targets[consumer..], files);
            // The consumer reads its items only through these walks.
            if let Some(items) = effects.targets[..consumer]
                .iter_mut()
                .find(|target| target.via == Via::Items)
            {
                items.bounds = walks
                    .iter()
                    .map(|walk| walk.bounds.clone())
                    .collect::<Option<Vec<_>>>()
                    .map(|bounds| bounds.concat());
            }
            effects.targets.extend(walks);
        }
        Some(None) => effects.hidden_content = true,
        None => {}
    }
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

fn infer_wrapper_files(
    command: &CommandRecord,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    for (index, file) in command.wrapper_files.iter().enumerate() {
        let cwd = crate::record::entered(
            cwd,
            &command.argv,
            &command.wrapper_files[..index],
            host.home,
        );
        let mut target = Target::from_word(
            &file.word(&command.argv),
            &cwd,
            host,
            file.effect,
            Walk::None,
        );
        target.via = Via::Option;
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

/// The hidden walk an item consumer reads through: `None` when it reads
/// none, `Some(None)` when the walk may hand it any name, and otherwise the
/// walks narrowed to the names the producers' bounds allow.
fn infer_items(
    program: &str,
    command: &CommandRecord,
    host: HostFacts<'_>,
    effects: &mut Effects,
) -> Option<Option<(Vec<Target>, bool)>> {
    if let Some(items) = &command.items {
        let effect = match program {
            "echo" | "printf" | "print" | ":" | "true" | "false" | "export" | "set" | "unset"
            | "typeset" | "declare" | "local" | "cd" => Effect::Name,
            "rm" | "mv" | "ln" | "stat" | "test" | "[" | "chmod" | "chown" | "chgrp"
            | "chflags" | "touch" | "rmdir" | "mkdir" | "wc" | "file" | "shasum" | "sha1sum"
            | "sha256sum" | "md5" | "md5sum" | "cksum" | "realpath" | "readlink" | "basename"
            | "dirname" => Effect::Meta,
            "ls" | "tree" | "du" | "find" | "fd" => Effect::List,
            "pushd" | "popd" => Effect::Enter,
            _ => clients::operand(program, &command.argv[command.program.unwrap_or(0) + 1..])
                .unwrap_or(Effect::Read),
        };
        let walk = if items.listed == Listed::Visible {
            Walk::Visible
        } else {
            Walk::Hidden
        };
        effects
            .targets
            .push(Target::new(items.root.clone(), effect, walk, Via::Items));
        (effect == Effect::Read && walk == Walk::Hidden).then(|| match &items.listed {
            Listed::Find(runs) => runs
                .iter()
                .map(|run| find_printed_walks(run, host, effects))
                .collect::<Option<Vec<_>>>()
                .map(|runs| {
                    let files = runs.iter().all(|(_, files)| *files);
                    (
                        runs.into_iter().flat_map(|(walks, _)| walks).collect(),
                        files,
                    )
                }),
            _ => None,
        })
    } else {
        None
    }
}

fn infer_xargs(program: &str, command: &CommandRecord, effects: &mut Effects) {
    if command.wrappers.iter().any(|w| w == "xargs") {
        effects.consumes_listing = xargs_content_consumer(program);
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

/// Reads each argument from the first one whose role the grammar cannot
/// prove, keeping no claim that would narrow it to a name, value or
/// destination; the owner's own observations stay alongside. The owner
/// already gave that first word its positional role unless its
/// representative reads as an option.
fn infer_unproven(args: &[Word], cwd: &str, host: HostFacts<'_>, effects: &mut Effects) {
    let positional = args.first().is_some_and(|word| !word.starts_with('-'));
    for arg in &args[usize::from(positional)..] {
        if let Some(value) = operand_value(arg) {
            effects.targets.push(Target::from_word(
                &arg.with_text(value.into()),
                cwd,
                host,
                Effect::Read,
                Walk::Visible,
            ));
        }
    }
}

fn infer_completion(program: &str, args: &[Word], command: &CommandRecord, effects: &mut Effects) {
    // A repetition projects each literal, not every possible argument sequence.
    // Positional roles and executable text cannot use those projections as a
    // complete command. Independent operand owners above can check their union;
    // transformations also need a complete resource spelling. A word with no
    // representative text spells nothing, and its owner already leaves it
    // unresolved. Code that its source owner observed was re-expanded there.
    let mut repeated = args
        .iter()
        .filter(|word| word.cardinality_unknown && word.role != Role::ObservedShellCode);
    if !effects.independent_arguments && repeated.clone().next().is_some()
        || repeated.any(|word| word.expands && !word.text.is_empty())
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
