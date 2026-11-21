use super::*;
use crate::record::WalkRun;
use std::collections::BTreeMap;

/// What a listing's arguments say about its walk.
struct Listing {
    /// The paths it walks.
    paths: Vec<Word>,
    /// The directory it resolves them in.
    base: String,
    /// Where an fd command to run starts.
    child_at: Option<usize>,
    /// Whether it shows hidden names.
    hidden: bool,
    /// Where a find expression starts.
    expression: usize,
    /// The names its filters let through to each command and to its output.
    bounds: Option<find::Bounds>,
}

pub(super) fn infer_listing(
    program: &str,
    args: &[Word],
    command: &CommandRecord,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
    depth: usize,
) {
    let Listing {
        paths,
        base,
        child_at,
        hidden,
        expression,
        bounds,
    } = listing_arguments(program, args, cwd, host, effects);
    if program == "find" {
        find_outputs(args, expression, cwd, host, effects);
    }
    let item_root = paths
        .first()
        .map_or_else(|| base.clone(), |path| path.text.clone());
    for path in &paths {
        effects.targets.push(Target::from_word(
            path,
            cwd,
            host,
            Effect::List,
            Walk::Visible,
        ));
    }
    // A name bound limits what a walk hands on to the files it can name, as
    // a search filter limits a walk, so those files are read instead.
    let children: Vec<(usize, usize)> = if let Some(index) = child_at {
        vec![(index, args.len())]
    } else if program == "find" {
        args.iter()
            .enumerate()
            .filter(|(_, arg)| ["-exec", "-execdir", "-ok", "-okdir"].contains(&arg.as_str()))
            .map(|(index, _)| {
                (
                    index + 1,
                    args[index + 1..]
                        .iter()
                        .position(|arg| arg == ";" || arg == "+")
                        .map_or(args.len(), |end| index + 1 + end),
                )
            })
            .collect()
    } else {
        Vec::new()
    };
    for (start, end) in children {
        let mut nested = child(command, &args[start..end], &base);
        // A batch placeholder becomes one argument per match, so the arguments
        // from it onward lose their written positions.
        if program == "fd" && ["-X", "--exec-batch"].contains(&args[start - 1].as_str()) {
            let placeholders = ["{}", "{/}", "{//}", "{.}", "{/.}"];
            if let Some(first) = nested.argv.iter().position(|word| {
                placeholders
                    .iter()
                    .any(|placeholder| word.contains(placeholder))
            }) {
                for word in &mut nested.argv[first..] {
                    word.fixed_position = false;
                }
            }
        }
        if program == "fd" {
            nested.items = Some(crate::record::Items {
                root: item_root.clone(),
                listed: crate::record::Listed::Visible,
            });
        }
        let result = infer_at(&nested, &base, host, depth + 1);
        if hidden
            && args.get(start).is_some_and(|name| content_consumer(name))
            && let Some(find::Handed {
                names: Some(names),
                files,
            }) = bounds
                .as_ref()
                .and_then(|bounds| bounds.execs.get(&(start - 1)))
        {
            effects.targets.extend(handed(
                paths
                    .iter()
                    .map(|path| narrowed_walk(path, names, cwd, host))
                    .collect(),
                &result.targets,
                *files,
            ));
        }
        effects.targets.extend(result.targets);
        effects.gaps.extend(result.gaps);
        effects.code.extend(result.code);
        effects.inline |= result.inline;
    }
}

/// Narrowed walks whose names a consumer reads as it reads its own operands:
/// a recursive consumer walks below each name it is handed, so the names no
/// longer bound the files it reads, unless `files` says every name is a
/// regular file.
pub(super) fn handed(mut walks: Vec<Target>, consumer: &[Target], files: bool) -> Vec<Target> {
    let walk = consumer
        .iter()
        .map(|target| target.walk)
        .max()
        .filter(|_| !files)
        .unwrap_or(Walk::None);
    for target in &mut walks {
        if walk != Walk::None {
            target.bounds = None;
        }
        for read in target.narrowed.iter_mut().flatten() {
            read.walk = walk;
        }
    }
    walks
}

fn content_consumer(name: &str) -> bool {
    [
        "cat", "head", "tail", "less", "more", "bat", "sed", "awk", "jq", "yq", "base64", "xxd",
        "od", "strings", "sort", "uniq", "cut", "nl", "sh", "bash", "zsh", "python", "python3",
        "node", "ruby", "perl", "grep", "rg", "ag", "ack",
    ]
    .contains(&name)
}

/// The hidden names a walk prints.
pub(super) enum Printed {
    NoHidden,
    /// It may print any hidden name.
    Unbounded,
    /// The walks that read them, narrowed to the names its filters bound, and
    /// whether it prints only regular files.
    Bounded(Vec<Target>, bool),
}

pub(super) fn printed_walks(run: &WalkRun, host: HostFacts<'_>) -> Printed {
    // The producer's own effects belong to its own command record, and its
    // argv positions mean nothing to the consumer.
    let Listing {
        mut paths,
        base,
        hidden,
        bounds,
        ..
    } = listing_arguments(
        &run.program,
        &run.args,
        &run.cwd,
        host,
        &mut Effects::default(),
    );
    if !hidden {
        return Printed::NoHidden;
    }
    let Some(find::Handed {
        names: Some(names),
        files,
    }) = bounds.map(|bounds| bounds.printed)
    else {
        return Printed::Unbounded;
    };
    if paths.is_empty() {
        paths.push(Word::literal(base));
    }
    Printed::Bounded(
        paths
            .iter()
            .map(|path| narrowed_walk(path, &names, &run.cwd, host))
            .collect(),
        files,
    )
}

/// Where a find expression starts, after its options and paths.
fn find_expression(args: &[Word]) -> usize {
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "-f" {
            index += 2;
            continue;
        }
        // Runtime text before the expression may be a path, so only fixed
        // text starts the expression.
        if arg != "--"
            && !arg.option_unknown()
            && arg.starts_with(['-', '(', '!'])
            && !arg
                .strip_prefix('-')
                .and_then(|flags| flags.chars().next())
                .is_some_and(|flag| "HLPEXxdsO".contains(flag))
        {
            return index;
        }
        index += 1;
    }
    args.len()
}

fn listing_arguments(
    program: &str,
    args: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) -> Listing {
    if program == "find" {
        return find_arguments(args, cwd);
    }
    // fd runs everything after an exec option as the command.
    let child_at = (program == "fd")
        .then(|| {
            args.iter()
                .position(|arg| ["-x", "-X", "--exec", "--exec-batch"].contains(&arg.as_str()))
        })
        .flatten();
    let options = &args[..child_at.unwrap_or(args.len())];
    let binding = crate::options::bind(
        options,
        match program {
            "fd" => &grammars::FD,
            "du" => &grammars::DU,
            _ => &grammars::TREE,
        },
    );
    if let Some(start) = binding.unproven {
        effects.unproven.get_or_insert(start);
    }
    let mut base = cwd.to_owned();
    let mut paths = Vec::new();
    for bound in &binding.values {
        let value = bound.value.word(options);
        match bound.value.effect {
            Effect::Enter => base = at(&value, &base),
            Effect::List => paths.push(value.with_text(at(&value, &base))),
            Effect::Name => {}
            effect => {
                let mut target = Target::from_word(&value, cwd, host, effect, Walk::None);
                target.via = Via::Option;
                effects.targets.push(target);
            }
        }
    }
    let hidden = ["-H", "--hidden", "-u", "--unrestricted"]
        .iter()
        .chain(if program == "fd" {
            &[][..]
        } else {
            &["-a"][..]
        })
        .any(|flag| binding.has(flag));
    // tree --fromfile reads a listing from its operands instead of walking them.
    let fromfile = program == "tree" && (binding.has("--fromfile") || binding.has("--fromtabfile"));
    // After a word the grammar cannot place, no operand becomes fd's pattern.
    let pattern = program == "fd"
        && binding
            .operands
            .first()
            .is_some_and(|&first| binding.unproven.is_none_or(|start| first < start));
    for &index in &binding.operands[usize::from(pattern)..] {
        let arg = &options[index];
        if fromfile {
            let mut target = Target::from_word(arg, cwd, host, Effect::Read, Walk::None);
            target.via = Via::Option;
            effects.targets.push(target);
        } else {
            paths.push(arg.with_text(at(arg, &base)));
        }
    }
    if paths.is_empty() && !fromfile {
        paths.push(Word::literal(base.clone()));
    }
    let bounds = (program == "fd").then(|| {
        fd_bounds(
            &binding,
            options,
            pattern.then(|| &options[binding.operands[0]]),
            child_at,
        )
    });
    Listing {
        paths,
        base,
        child_at: child_at.map(|index| index + 1),
        hidden,
        expression: args.len(),
        bounds,
    }
}

/// fd hands on only names that match its glob pattern and one of its
/// extensions, so each of them bounds the names; a regex pattern does not.
fn fd_bounds(
    binding: &crate::options::Binding,
    options: &[Word],
    pattern: Option<&Word>,
    exec: Option<usize>,
) -> find::Bounds {
    let known = |word: &Word| !word.expands && !word.runtime_unknown;
    let values = |names: [&str; 2]| {
        binding
            .values
            .iter()
            .filter(move |bound| names.contains(&bound.name))
            .map(|bound| bound.value.word(options))
            .collect::<Vec<_>>()
    };
    // The last of `--glob` and `--regex` decides how fd reads its pattern.
    let glob = binding
        .flags
        .iter()
        .rev()
        .find(|flag| ["-g", "--glob", "--regex"].contains(&flag.as_str()))
        .is_some_and(|flag| flag != "--regex");
    let mut names = pattern.filter(|_| glob).and_then(find::filter_names);
    let extensions = values(["-e", "--extension"]);
    if !extensions.is_empty() && extensions.iter().all(known) {
        names.get_or_insert_with(Vec::new).extend(
            extensions
                .iter()
                .map(|extension| format!("*.{}", extension.trim_start_matches('.'))),
        );
    }
    let types = values(["-t", "--type"]);
    let handed = find::Handed {
        names,
        files: !types.is_empty()
            && types
                .iter()
                .all(|kind| matches!(kind.exact(), Some("f" | "file"))),
    };
    match exec {
        // The command's output replaces the names fd would print.
        Some(index) => find::Bounds {
            execs: BTreeMap::from([(index, handed)]),
            printed: find::Handed::default(),
        },
        None => find::Bounds {
            execs: BTreeMap::new(),
            printed: handed,
        },
    }
}

/// The paths before a find expression and where the expression starts.
fn find_arguments(args: &[Word], cwd: &str) -> Listing {
    let expression = find_expression(args);
    let mut paths = Vec::new();
    let mut index = 0;
    while index < expression {
        let arg = &args[index];
        if arg == "-f" {
            paths.extend(args.get(index + 1).cloned());
            index += 1;
        } else if arg.option_unknown() || !arg.starts_with('-') {
            paths.push(arg.with_text(at(arg, cwd)));
        }
        index += 1;
    }
    if paths.is_empty() {
        paths.push(Word::literal(cwd.to_owned()));
    }
    Listing {
        paths,
        base: cwd.to_owned(),
        child_at: None,
        hidden: true,
        expression,
        bounds: find::bounds(args, expression),
    }
}

/// The files the output primaries of a find expression write.
fn find_outputs(
    args: &[Word],
    expression: usize,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    for (index, arg) in args.iter().enumerate().skip(expression) {
        if ["-fprint", "-fprint0", "-fls", "-fprintf"].contains(&arg.as_str())
            && let Some(file) = args.get(index + 1)
        {
            let mut target = Target::from_word(file, cwd, host, Effect::Write, Walk::None);
            target.via = Via::Option;
            effects.targets.push(target);
        }
    }
}

mod find;
mod grammars;
