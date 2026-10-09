use super::super::*;

pub(super) fn output(
    program: &str,
    args: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
    command: &CommandRecord,
) {
    effects.independent_arguments = args.iter().all(|arg| !arg.starts_with('-'));
    effects.targets.extend(
        args.iter()
            .filter(|arg| arg.globs && !arg.starts_with('-'))
            .map(|arg| Target::from_word(arg, cwd, host, Effect::Name, Walk::None)),
    );
    effects.variable = !(program != "echo" && args.first().is_some_and(|arg| arg == "-v"))
        && command.variables().any(|name| secret_name(name));
}

pub(super) fn metadata(
    program: &str,
    args: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    // Metadata and content-read roles have different credential decisions;
    // visible walks still check protected App Data ownership.
    let walk = if ["df", "stat", "test", "[", "mkdir", "readlink"].contains(&program) {
        Walk::None
    } else {
        Walk::Visible
    };
    effects.targets.extend(args.iter().filter_map(|word| {
        operand_value(word).map(|value| {
            Target::from_word(&word.with_text(value.into()), cwd, host, Effect::Meta, walk)
        })
    }));
}

pub(super) fn directory(
    program: &str,
    args: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    let mut options = true;
    for word in args {
        if options && word == "--" {
            options = false;
        } else if !options || !word.starts_with('-') {
            effects.targets.push(Target::from_word(
                word,
                cwd,
                host,
                if program == "cd" {
                    Effect::Name
                } else {
                    Effect::Enter
                },
                Walk::None,
            ));
        }
    }
}

pub(super) fn temporary(
    program: &str,
    args: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    effects.gaps.push(CoverageGap::UnknownProgram {
        program: program.into(),
    });
    effects.targets.extend(args.iter().filter_map(|word| {
        operand_value(word).map(|value| {
            Target::from_word(
                &word.with_text(value.into()),
                cwd,
                host,
                Effect::Write,
                Walk::None,
            )
        })
    }));
}

pub(super) fn infer(
    program: &str,
    args: &[Word],
    command: &CommandRecord,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    let index = command.program.unwrap_or(0);
    match program {
        "printf" | "echo" | "print" => output(program, args, cwd, host, effects, command),
        "true" | "false" | ":" | "unset" | "local" | "break" | "continue" | "return" | "shift" => {
            effects.independent_arguments = matches!(program, "true" | "false" | ":" | "local")
                && args.iter().any(|arg| arg.cardinality_unknown)
                && args.iter().all(|arg| !arg.starts_with('-'));
        }
        "tr" => {}
        "kill" => {
            effects.independent_arguments = true;
            effects.targets.extend(
                args.iter()
                    .map(|arg| Target::from_word(arg, cwd, host, Effect::Name, Walk::None)),
            );
        }
        "mktemp" => temporary(program, args, cwd, host, effects),
        "cd" | "pushd" | "popd" => directory(program, args, cwd, host, effects),
        "hash" => effects.independent_arguments = true,
        "setopt" | "unsetopt" | "emulate" => {
            effects.gaps.push(
                if command.shell && command.argv[..index].iter().all(|w| w.contains('=')) {
                    CoverageGap::ExecutorDivergence
                } else {
                    CoverageGap::UnknownProgram {
                        program: program.into(),
                    }
                },
            );
        }
        "set" => {
            effects.independent_arguments = args.first().is_some_and(|word| word == "--");
            effects.dump = command.shell && !command.argv[index].contains('/') && args.is_empty()
        }
        "typeset" | "declare" => {
            effects.independent_arguments = args.iter().any(|arg| arg.cardinality_unknown)
                && args.iter().all(|arg| arg.contains('='));
            effects.dump = args.is_empty()
                || args.len() == 1 && args[0].starts_with('-') && args[0].contains(['p', 'x']);
            effects.variable = args
                .iter()
                .any(|arg| !arg.starts_with('-') && !arg.contains('=') && secret_name(arg));
        }
        "printenv" => {
            effects.dump = args.is_empty();
            effects.variable = args.iter().any(|s| secret_name(s));
        }
        "export" => {
            effects.independent_arguments = args.iter().any(|arg| arg.cardinality_unknown)
                && args.iter().all(|arg| arg.contains('='));
            effects.dump =
                args.is_empty() || args.iter().any(|s| s.starts_with('-') && s.contains('p'))
        }
        _ => metadata(program, args, cwd, host, effects),
    }
}
