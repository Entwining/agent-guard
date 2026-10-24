use super::super::*;
use super::read;

pub(super) fn listing(args: &[Word], cwd: &str, host: HostFacts<'_>, effects: &mut Effects) {
    effects.independent_arguments = independent_operands(args, args);
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
    effects.independent_arguments = independent_operands(args, args);
    if command.unresolved() {
        effects.gaps.push(CoverageGap::UnresolvedTarget);
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
        claimed.extend(index..index + data);
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

// The options of macOS and GNU diff, from their manuals.
const DIFF: Getopt = Getopt {
    short_flags: "aBbcdEefHilNnPpqrsTtuwyZ",
    short_values: "ACDFILSUWXx",
    flags: &[
        "--brief",
        "--ed",
        "--expand-tabs",
        "--forward-ed",
        "--ignore-all-space",
        "--ignore-case",
        "--ignore-space-change",
        "--ignore-blank-lines",
        "--ignore-tab-expansion",
        "--ignore-trailing-space",
        "--initial-tab",
        "--minimal",
        "--no-dereference",
        "--normal",
        "--rcs",
        "--show-c-function",
        "--speed-large-files",
        "--strip-trailing-cr",
        "--text",
        "--unidirectional-new-file",
        "--new-file",
        "--recursive",
        "--report-identical-files",
        "--side-by-side",
        "--suppress-common-lines",
        "--suppress-blank-empty",
        "--left-column",
        "--paginate",
        "--ignore-file-name-case",
        "--no-ignore-file-name-case",
        "--color",
        "--help",
        "--version",
    ],
    values: &[
        "--algorithm",
        "--context",
        "--ifdef",
        "--unified",
        "--show-function-line",
        "--ignore-matching-lines",
        "--label",
        "--width",
        "--tabsize",
        "--starting-file",
        "--exclude",
        "--exclude-from",
        "--changed-group-format",
        "--new-group-format",
        "--old-group-format",
        "--unchanged-group-format",
        "--line-format",
        "--new-line-format",
        "--old-line-format",
        "--unchanged-line-format",
        "--horizon-lines",
        "--from-file",
        "--to-file",
        "--palette",
    ],
};

/// The name patterns of `diff -x` and `--exclude`, which select the
/// directory entries a recursive comparison skips.
pub(super) fn diff_patterns(args: &[Word]) -> Vec<usize> {
    option_values(args, &DIFF, &["-x", "--exclude"]).0
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
        let Some(arg) = arg.exact() else {
            // Text the shell supplies could spell an option that consumes
            // the next word, unless its known start rules one out.
            if !arg.option_unknown() {
                continue;
            }
            break;
        };
        if arg == "--" {
            break;
        }
        let value = match arg.split_once('=') {
            Some((key, _)) if options.contains(&key) => index,
            None if options.contains(&arg) => index + 1,
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
        .map_while(Word::exact)
        .take_while(|word| !word.starts_with('-'))
        .collect();
    GH_FORMATTED_COMMANDS.split(',').any(|command| {
        let command: Vec<&str> = command.split(' ').collect();
        path.starts_with(&command)
    })
}

/// The options of a client whose parser accepts them anywhere before `--`:
/// a value option takes the next word, or a value glued as `--name=value`
/// or `-nvalue`.
struct Grammar {
    flags: &'static [&'static str],
    values: &'static [&'static str],
}

struct Parsed {
    positionals: Vec<usize>,
    /// How many positionals precede `--`; only those can name a subcommand.
    commands: usize,
    /// Each option, with the index of the word holding its value, which is
    /// past the last word when the value is missing.
    options: Vec<(&'static str, Option<usize>)>,
}

impl Parsed {
    /// The word at positional `at`, when the client is sure to take the whole
    /// word as that positional: a word the shell may split could also hold
    /// the next operands.
    fn positional(&self, args: &[Word], at: usize) -> Option<usize> {
        let index = *self.positionals.get(at)?;
        args[index].fixed_position.then_some(index)
    }
    /// The subcommand a positional spells, when the shell fixes its text.
    fn command<'w>(&self, args: &'w [Word], at: usize) -> Option<&'w str> {
        (at < self.commands).then(|| args[self.positionals[at]].exact())?
    }
}

/// Where `grammar` places each word, or None where it cannot: at an option
/// spelling it does not define, such as an unknown option or grouped short
/// options, whose value count it cannot tell, or before `--` at a word the
/// shell may expand into several or whose runtime text could spell an
/// option.
fn parse(args: &[Word], grammar: &Grammar) -> Option<Parsed> {
    let mut positionals = Vec::new();
    let mut separator = None;
    let mut options = Vec::new();
    let mut index = 0;
    while let Some(word) = args.get(index) {
        index += 1;
        if separator.is_some() {
            positionals.push(index - 1);
            continue;
        }
        if !word.fixed_position {
            return None;
        }
        let (word, exact) = match word.exact() {
            Some(text) => (text, true),
            None if !word.option_unknown() => {
                positionals.push(index - 1);
                continue;
            }
            // Of runtime text, only a known option name with a glued value
            // is sure to consume no other word.
            None => (word.known_prefix(), false),
        };
        // pflag and clap parse a lone `-` as an operand.
        if exact && (word == "-" || !word.starts_with('-')) {
            positionals.push(index - 1);
            continue;
        }
        if word == "--" {
            separator = Some(positionals.len());
            continue;
        }
        let (name, glued) = match word.split_once('=') {
            Some((name, _)) if name.starts_with("--") => (name, true),
            _ if !word.starts_with("--") && word.len() > 2 => (word.get(..2)?, true),
            _ => (word, false),
        };
        if !exact && !glued {
            return None;
        }
        let find = |table: &[&'static str]| table.iter().copied().find(|option| *option == name);
        if let Some(option) = find(grammar.values) {
            if !glued {
                if args.get(index).is_some_and(|value| !value.fixed_position) {
                    return None;
                }
                index += 1;
            }
            options.push((option, Some(index - 1)));
        } else if let Some(option) = find(grammar.flags)
            && (!glued || option.starts_with("--"))
        {
            options.push((option, None));
        } else {
            return None;
        }
    }
    Some(Parsed {
        commands: separator.unwrap_or(positionals.len()),
        positionals,
        options,
    })
}

// Rex's Cobra commands `api describe`, `api call`, `block call` and `do`
// with the global options, from `rex help`.
const REX: Grammar = Grammar {
    flags: &[
        "-h",
        "--help",
        "-v",
        "--verbose",
        "--autostart",
        "--json",
        "--all",
        "--fail-fast",
    ],
    values: &[
        "-S",
        "--server",
        "-C",
        "--client",
        "--timeout",
        "--color",
        "-s",
        "--session",
        "-b",
        "--block",
        "-w",
        "--window",
        "-e",
        "--eval",
        "-a",
        "--action",
        "--args",
        "--origin",
        "--for",
    ],
};

/// The METHOD of `rex api describe`, `rex api call` and `rex block call`,
/// and the ACTION of `rex do`. Their ARGS, `KEY=VALUE` pairs and option
/// values may name files an action opens, so they keep the unknown role.
pub(super) fn rex_names(args: &[Word]) -> Vec<usize> {
    let Some(parsed) = parse(args, &REX) else {
        return Vec::new();
    };
    let slot = match (parsed.command(args, 0), parsed.command(args, 1)) {
        (Some("api"), Some("describe" | "call")) | (Some("block"), Some("call")) => 2,
        (Some("do"), _) if performs_action(args, &parsed) => 1,
        _ => return Vec::new(),
    };
    parsed.positional(args, slot).into_iter().collect()
}

/// Whether `rex do` takes its first positional as an action name, or `-`
/// for a script on standard input, rather than a file it opens: a path with
/// a slash or a `.lua` suffix names a script, as does any first positional
/// with `--action`, and `-e` supplies the code itself. A positional whose
/// text is unknown could still spell a script path or one of those options.
fn performs_action(args: &[Word], parsed: &Parsed) -> bool {
    let Some(&selector) = parsed.positionals.get(1) else {
        return false;
    };
    let selector = args[selector].as_str();
    !parsed
        .options
        .iter()
        .any(|(option, _)| ["-e", "--eval", "-a", "--action"].contains(option))
        && parsed
            .positionals
            .iter()
            .all(|&index| args[index].exact().is_some())
        && !selector.contains('/')
        && !selector.to_ascii_lowercase().ends_with(".lua")
}

// Sessidx's clap options, from its `src/main.rs` and `query::Filters`.
// `--by` takes an optional value, which this grammar leaves positional;
// `count` claims no positional.
const SESSIDX: Grammar = Grammar {
    flags: &["-h", "--help", "-V", "--version", "--full", "--by"],
    values: &[
        "--db",
        "--root",
        "--agent",
        "--role",
        "--session-kind",
        "--since",
        "--until",
        "--cwd",
        "--session",
        "--limit",
        "--cursor",
        "--around",
        "--program",
    ],
};

/// The query of `sessidx search`, `grep` and `sql`, and the `--cwd` filter of
/// `search`, `grep` and `count`, which sessidx compares with indexed session
/// directories without opening it. `show` reads the transcript it names, and
/// `--db` and `--root` are files sessidx opens.
pub(super) fn sessidx_queries(args: &[Word]) -> Vec<usize> {
    let Some(parsed) = parse(args, &SESSIDX) else {
        return Vec::new();
    };
    let command = parsed.command(args, 0);
    let mut claimed = Vec::new();
    if matches!(command, Some("search" | "grep" | "sql")) {
        claimed.extend(parsed.positional(args, 1));
    }
    if matches!(command, Some("search" | "grep" | "count")) {
        claimed.extend(
            parsed
                .options
                .iter()
                .filter(|(option, _)| *option == "--cwd")
                .filter_map(|(_, value)| *value),
        );
    }
    claimed
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
