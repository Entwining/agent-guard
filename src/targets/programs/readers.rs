use super::super::*;
use super::read;

mod grammars;

pub(super) fn listing(args: &[Word], cwd: &str, host: HostFacts<'_>, effects: &mut Effects) {
    effects.independent_arguments = independent_operands(args, args);
    let mut recursive = false;
    let mut entries = false;
    let mut options = true;
    let mut paths = Vec::new();
    for arg in args {
        if options && arg == "--" {
            options = false;
        } else if options && arg.starts_with('-') && arg.len() > 1 {
            let short = !arg.starts_with("--");
            recursive |= arg == "--recursive" || short && arg.contains('R');
            entries |= arg == "--directory" || short && arg.contains('d');
        } else {
            paths.push(arg);
        }
    }
    let implicit = Word::literal(cwd.to_owned());
    if paths.is_empty() {
        paths.push(&implicit);
    }
    for path in paths {
        // `-d` shows each operand itself rather than its contents.
        let mut target = read(path, cwd, host, recursive && !entries);
        target.effect = if entries { Effect::Meta } else { Effect::List };
        effects.targets.push(target);
    }
}

/// A reader of the files its operands name, none of which it takes as an
/// `@FILE` convention.
pub(super) fn files(
    program: &str,
    args: &[Word],
    command: &CommandRecord,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) -> Operands {
    effects.independent_arguments = independent_operands(args, args);
    if command.unresolved() {
        effects.gaps.push(CoverageGap::UnresolvedTarget);
    }
    Operands {
        literal: true,
        ..options(program, args, Walk::None, cwd, host, effects)
    }
}

pub(super) fn filter(program: &str, args: &[Word]) -> Vec<usize> {
    // The command-line filter is data.
    let mut claimed: Vec<usize> = Vec::new();
    let mut positional = Vec::new();
    let mut options = true;
    let mut index = 0;
    while let Some(word) = args.get(index) {
        index += 1;
        let Some(arg) = word.exact() else {
            if !options || !word.option_unknown() {
                positional.push((index - 1, word));
                continue;
            }
            // Runtime text could be an option that consumes later words or,
            // like `-f`, makes the first operand a file.
            claimed.retain(|&index| args[index].fixed_position);
            return claimed;
        };
        if !options || !arg.starts_with('-') {
            positional.push((index - 1, word));
            continue;
        }
        if arg == "--" {
            options = false;
            continue;
        }
        // -f, alone or in a short-option group, and --run-tests make the
        // first operand a file to read rather than the filter.
        if ["--from-file", "--run-tests"].contains(&arg)
            || !arg.starts_with("--") && !arg.starts_with("-L") && arg.contains('f')
        {
            return Vec::new();
        }
        // An option value is never the filter; the files named by
        // --slurpfile, --rawfile and -L stay readable operands.
        let (values, data) = match (program, arg) {
            ("jq", "--arg" | "--argjson") => (2, 2),
            ("jq", "--slurpfile" | "--rawfile") => (2, 1),
            ("jq", "--indent") => (1, 1),
            ("jq", "-L" | "--library-path") => (1, 0),
            _ => (0, 0),
        };
        claimed.extend(index..(index + data).min(args.len()));
        index += values;
    }
    let mut operands = positional.into_iter();
    let first = operands.next();
    let filter = match first {
        Some((_, arg)) if program == "yq" => match arg.exact() {
            Some("eval" | "e" | "eval-all" | "ea") => operands.next(),
            Some(_) => first,
            // Runtime text could name a subcommand.
            None => None,
        },
        _ => first,
    };
    if let Some((index, _)) = filter {
        claimed.push(index);
    }
    claimed.retain(|&index| args[index].fixed_position);
    claimed
}

/// The generic operand roles of a reader whose operands `walk` reads: the
/// options of a modelled grammar claim their words and take the roles it
/// declares, and its operands are literal names.
pub(super) fn options(
    program: &str,
    args: &[Word],
    walk: Walk,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) -> Operands {
    let grammar = match program {
        "sed" => &grammars::SED,
        "awk" => &grammars::AWK,
        "sort" => &grammars::SORT,
        "diff" => &grammars::DIFF,
        "base64" => &grammars::BASE64,
        "hexdump" => &grammars::HEXDUMP,
        "openssl" => &grammars::OPENSSL,
        _ => {
            return Operands {
                walk,
                claimed: Vec::new(),
                literal: false,
            };
        }
    };
    let binding = crate::options::bind(args, grammar);
    // Without a script option, sed's first operand is its script.
    let script = (program == "sed"
        && !binding
            .values
            .iter()
            .any(|bound| ["-e", "--expression", "-f", "--file"].contains(&bound.name)))
    .then(|| binding.operands.first().copied())
    .flatten();
    let mut claimed = option_targets(args, binding, walk, cwd, host, effects);
    claimed.extend(script);
    Operands {
        walk,
        claimed,
        literal: true,
    }
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
