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
    secrets::infer(program, args, &mut effects);
    if let Some(operands) = programs::infer(program, args, command, cwd, host, &mut effects, depth)
    {
        infer_operands(program, args, cwd, host, &mut effects, &operands);
    }
    if let Some(start) = effects.unproven {
        infer_unproven(&args[start..], cwd, host, &mut effects);
    }
    if let Some((walks, files, bounded)) = hidden_items_read {
        let walks = listing::handed(walks, &effects.targets[consumer..], files);
        // The consumer reads its items only through these walks.
        if bounded
            && let Some(items) = effects.targets[..consumer]
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
    infer_completion(program, args, command, &mut effects);
    infer_cwd(cwd_program, cwd, &mut effects);
    // A program Rex starts writes to the terminal Rex opens, not to the
    // shell's redirections or pipe.
    if command.wrappers.iter().any(|w| w == "rex") {
        effects.variable_stdout_only = false;
    }
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
    let args = &command.argv[index + 1..];
    match &command.stdin {
        crate::record::Stdin::Code => {
            effects.inline |= command
                .redirects
                .iter()
                .any(|r| matches!(r.direction, Direction::Heredoc | Direction::Herestring));
        }
        crate::record::Stdin::InterpreterStream(texts)
            if reads_program_from_stdin(program, args) =>
        {
            effects.inline |= !texts.is_empty();
        }
        _ => {}
    }
}

/// The walks through which an item consumer reads the hidden names its
/// producers print, narrowed to the names their filters bound; whether those
/// producers print only regular files; and whether every producer is bounded,
/// so that the walks are all it reads. `None` when it reads no hidden name.
fn infer_items(
    program: &str,
    command: &CommandRecord,
    host: HostFacts<'_>,
    effects: &mut Effects,
) -> Option<(Vec<Target>, bool, bool)> {
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
            // Only a modelled program's operands are files it reads, as for
            // the operands written on its command line.
            _ => clients::operand(program, &command.argv[command.program.unwrap_or(0) + 1..])
                .unwrap_or(if modelled_program(program) {
                    Effect::Read
                } else {
                    Effect::Name
                }),
        };
        let printed: Vec<_> = match &items.listed {
            Listed::Visible => Vec::new(),
            Listed::Walks(runs) => runs.iter().map(|run| printed_walks(run, host)).collect(),
        };
        let hidden = printed
            .iter()
            .any(|printed| !matches!(printed, Printed::NoHidden));
        let walk = if hidden { Walk::Hidden } else { Walk::Visible };
        effects
            .targets
            .push(Target::new(items.root.clone(), effect, walk, Via::Items));
        if effect != Effect::Read || !hidden {
            return None;
        }
        let bounded = printed
            .iter()
            .all(|printed| matches!(printed, Printed::Bounded(..)));
        let mut walks = Vec::new();
        let mut files = true;
        for printed in printed {
            if let Printed::Bounded(narrowed, regular) = printed {
                walks.extend(narrowed);
                files &= regular;
            }
        }
        Some((walks, files, bounded))
    } else {
        None
    }
}

fn infer_operands(
    program: &str,
    args: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
    operands: &Operands,
) {
    // Unclaimed glued values reach the generic operand role; program-specific
    // adapters retain ownership of the arguments they already claimed.
    for (index, arg) in args.iter().enumerate() {
        if operands.claimed.contains(&index) || arg.as_str() == "__observed_stream__" {
            continue;
        }
        let literal = arg.role == Role::Path || operands.literal && !arg.starts_with('-');
        for value in operand_values(arg, literal) {
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
                operands.walk,
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
        for value in operand_values(arg, false) {
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
            .any(|name| secret_variable(name));
}

fn infer_cwd(cwd_program: &str, cwd: &str, effects: &mut Effects) {
    // A modelled program that names no target works in the command cwd; what
    // an unknown program does there only the program decides.
    let named = effects.targets.iter().any(|target| {
        matches!(target.via, Via::Operand | Via::Cwd | Via::Scan)
            && !matches!(target.effect, Effect::Enter | Effect::Name)
    });
    if !DATA_PROGRAMS
        .split_whitespace()
        .any(|name| name == cwd_program)
        && !["cd", "pushd", "popd"].contains(&cwd_program)
        && !named
        && modelled_program(cwd_program)
    {
        effects.targets.push(Target::new(
            cwd.to_owned(),
            Effect::Enter,
            Walk::None,
            Via::Cwd,
        ));
    }
}
