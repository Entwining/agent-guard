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
    // The command-line filter is data.
    let mut claimed = Vec::new();
    let mut positional = Vec::new();
    let mut options = true;
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        index += 1;
        if !options || !arg.starts_with('-') {
            positional.push((index - 1, arg));
            continue;
        }
        if arg == "--" {
            options = false;
            continue;
        }
        // -f, alone or in a short-option group, and --run-tests make the
        // first operand a file to read rather than the filter.
        if ["--from-file", "--run-tests"].contains(&arg.as_str())
            || !arg.starts_with("--") && !arg.starts_with("-L") && arg.contains('f')
        {
            return Vec::new();
        }
        // An option value is never the filter; the files named by
        // --slurpfile, --rawfile and -L stay readable operands.
        let (values, data) = match (program, arg.as_str()) {
            ("jq", "--arg" | "--argjson") => (2, 2),
            ("jq", "--slurpfile" | "--rawfile") => (2, 1),
            ("jq", "--indent") => (1, 1),
            ("jq", "-L" | "--library-path") => (1, 0),
            _ => (0, 0),
        };
        claimed.extend(index..index + data);
        index += values;
    }
    let mut operands = positional.into_iter();
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
    claimed.retain(|&index| args[index].fixed_position);
    claimed
}

// gh parses its output formats only in `gh api` and the commands with
// `--json`; other commands reject them and extensions receive them unparsed.
// None of these commands reads a local file operand.
const GH_FORMATTED_COMMANDS: &str = "api,agent-task list,agent-task view,auth status,cache list,codespace list,codespace ports,codespace view,discussion list,discussion view,extension search,issue list,issue status,issue view,label list,pr checks,pr list,pr status,pr view,release list,release view,repo list,repo read-dir,repo read-file,repo view,repo autolink list,repo autolink view,repo deploy-key list,run list,run view,search code,search commits,search issues,search prs,search repos,secret list,skill list,skill search,variable get,variable list,workflow list";

pub(super) fn output_query(program: &str, args: &[Word]) -> Vec<usize> {
    // These options filter or format the client's own output.
    let options: &[&str] = match program {
        "gh" if gh_formats_output(args) => &["-q", "--jq", "-t", "--template"],
        "aws" => &["--query"],
        _ => &[],
    };
    let mut claimed = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        if arg == "--" {
            break;
        }
        let value = match arg.split_once('=') {
            Some((key, _)) if options.contains(&key) => index,
            None if options.contains(&arg.as_str()) => index + 1,
            _ => continue,
        };
        // A separate dash word means an earlier option may have consumed the
        // format option, leaving this word an option of its own.
        if args
            .get(value)
            .is_some_and(|word| (value == index || !word.starts_with('-')) && word.fixed_position)
        {
            claimed.push(value);
        }
    }
    claimed
}

fn gh_formats_output(args: &[Word]) -> bool {
    let path: Vec<&str> = args
        .iter()
        .map(Word::as_str)
        .take_while(|word| !word.starts_with('-'))
        .collect();
    GH_FORMATTED_COMMANDS.split(',').any(|command| {
        let command: Vec<&str> = command.split(' ').collect();
        path.starts_with(&command)
    })
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
