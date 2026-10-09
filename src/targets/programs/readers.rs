use super::super::*;
use super::read;

pub(super) fn listing(args: &[Word], cwd: &str, host: HostFacts<'_>, effects: &mut Effects) {
    effects.independent_arguments = independent_operands(args, args)
        && args
            .iter()
            .all(|arg| !arg.cardinality_unknown || arg.role != Role::Option(OptionRole::Name));
    let mut recursive = false;
    let mut options = true;
    let mut paths = Vec::new();
    for arg in args {
        if options && arg == "--" {
            options = false;
        } else if options && arg.starts_with('-') && arg.len() > 1 {
            recursive |= arg == "--recursive" || !arg.starts_with("--") && arg.contains('R');
        } else {
            paths.push(arg);
        }
    }
    effects.hidden_listing = args
        .iter()
        .any(|s| s.starts_with('-') && s.contains(['a', 'A']));
    let implicit = Word::literal(cwd.to_owned());
    if paths.is_empty() {
        paths.push(&implicit);
    }
    for path in paths {
        let mut target = read(path, cwd, host, recursive);
        target.effect = Effect::List;
        effects.targets.push(target);
    }
}

pub(super) fn files(args: &[Word], command: &CommandRecord, effects: &mut Effects) {
    effects.independent_arguments = independent_operands(args, args)
        && args
            .iter()
            .all(|arg| !arg.cardinality_unknown || arg.role != Role::Option(OptionRole::Name));
    if command.unresolved() {
        effects.gaps.push(CoverageGap::UnresolvedTarget);
    }
}

pub(super) fn filter(program: &str, args: &[Word]) -> Vec<usize> {
    let mut claimed = Vec::new();
    // The command-line filter is data; -f/--from-file names a readable file.
    if !args.iter().any(|arg| arg == "-f" || arg == "--from-file") {
        let mut operands = args
            .iter()
            .enumerate()
            .filter(|(_, arg)| !arg.starts_with('-'));
        let first = operands.next();
        let filter = if program == "yq"
            && first.is_some_and(|(_, arg)| ["eval", "e", "eval-all", "ea"].contains(&arg.as_str()))
        {
            operands.next()
        } else {
            first
        };
        if let Some((index, _)) = filter {
            claimed.push(index);
        }
    }
    claimed
}

pub(super) fn git(
    args: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
    command: &CommandRecord,
) {
    // Environment-supplied locations keep the corresponding option
    // roles, even when no location flag appears in argv.
    for (name, value) in &command.environment {
        let effect = match name.as_str() {
            "GIT_DIR" => Effect::Read,
            "GIT_WORK_TREE" => Effect::Enter,
            _ => continue,
        };
        if !value.text.is_empty() {
            let mut target = Target::from_word(
                value,
                cwd,
                host,
                effect,
                if args.iter().any(|arg| arg == "config") {
                    Walk::None
                } else {
                    Walk::Visible
                },
            );
            target.via = Via::Option;
            effects.targets.push(target);
        }
    }
    infer_git(args, cwd, host, effects);
}

pub(super) fn unknown(program: &str, args: &[Word], effects: &mut Effects) {
    effects.gaps.push(CoverageGap::UnknownProgram {
        program: program.to_owned(),
    });
    // Unknown width cannot move independent operands between roles
    // unless a candidate can become an option or consume its value.
    effects.independent_arguments = !modelled_program(program)
        && independent_operands(args, args)
        && args
            .iter()
            .all(|word| !word.cardinality_unknown || matches!(word.role, Role::Arg | Role::Path));
}
