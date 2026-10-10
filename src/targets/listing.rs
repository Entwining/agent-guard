use super::*;

pub(super) fn infer_listing(
    program: &str,
    args: &[Word],
    command: &CommandRecord,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
    depth: usize,
) {
    let (mut paths, base, child_at, hidden) = listing_arguments(program, args, cwd, host, effects);
    if paths.is_empty() {
        paths.push(Word::literal(base.clone()));
    }
    let item_root = paths[0].text.clone();
    for path in paths {
        effects.targets.push(Target::from_word(
            &path,
            cwd,
            host,
            Effect::List,
            Walk::Visible,
        ));
    }
    effects.hidden_listing = hidden;
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
                hidden: shows_hidden(args),
            });
        }
        let result = infer_at(&nested, &base, host, depth + 1);
        if effects.hidden_listing && args.get(start).is_some_and(|name| content_consumer(name)) {
            effects.hidden_content = true;
        }
        effects.targets.extend(result.targets);
        effects.gaps.extend(result.gaps);
        effects.code.extend(result.code);
        effects.inline.extend(result.inline);
        effects.hidden_content |= result.hidden_content;
    }
}

fn content_consumer(name: &str) -> bool {
    [
        "cat", "head", "tail", "less", "more", "bat", "sed", "awk", "jq", "yq", "base64", "xxd",
        "od", "strings", "sort", "uniq", "cut", "nl", "sh", "bash", "zsh", "python", "python3",
        "node", "ruby", "perl", "grep", "rg", "ag", "ack",
    ]
    .contains(&name)
}

fn listing_arguments(
    program: &str,
    args: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) -> (Vec<Word>, String, Option<usize>, bool) {
    let mut paths = Vec::new();
    let mut base = cwd.to_owned();
    let mut child_at = None;
    let mut skip = false;
    let mut hidden = program == "find";
    let mut pattern = program != "fd";
    for (index, arg) in args.iter().enumerate() {
        if arg.role == Role::Option(OptionRole::Name) {
            continue;
        }
        if skip {
            skip = false;
            continue;
        }
        if program == "fd" && ["-x", "-X", "--exec", "--exec-batch"].contains(&arg.as_str()) {
            child_at = Some(index + 1);
            break;
        }
        if program == "fd"
            && (arg == "-C"
                || arg.starts_with("--base-directory")
                || arg.starts_with("--search-path"))
        {
            let path = arg
                .split_once('=')
                .map(|(_, value)| arg.with_text(value.to_owned()))
                .or_else(|| args.get(index + 1).cloned());
            if let Some(path) = path {
                if arg == "-C" || arg.starts_with("--base-directory") {
                    base = at(&path, &base);
                } else {
                    paths.push(path.with_text(at(&path, &base)));
                }
            }
            skip = !arg.contains('=');
            continue;
        }
        if program == "du"
            && ["--exclude-from", "--files0-from"]
                .contains(&arg.split_once('=').map_or(arg.as_str(), |(key, _)| key))
        {
            let value = arg
                .split_once('=')
                .map(|(_, value)| arg.with_text(value.into()))
                .or_else(|| args.get(index + 1).cloned());
            if let Some(value) = value {
                let mut target = Target::from_word(&value, cwd, host, Effect::List, Walk::None);
                target.via = Via::Option;
                effects.targets.push(target);
            }
            skip = !arg.contains('=');
            continue;
        }
        if program == "find" && arg == "-f" {
            if let Some(path) = args.get(index + 1) {
                paths.push(path.clone());
            }
            skip = true;
            continue;
        }
        if program == "find" && arg == "--" {
            continue;
        }
        if program == "find"
            && arg.starts_with(['-', '(', '!'])
            && !arg
                .strip_prefix('-')
                .and_then(|flags| flags.chars().next())
                .is_some_and(|flag| "HLPEXxdsO".contains(flag))
        {
            break;
        }
        if let Some(flags) = arg.strip_prefix('-') {
            hidden |= arg == "--hidden"
                || arg == "--unrestricted"
                || !arg.starts_with("--")
                    && flags.chars().all(|letter| letter.is_ascii_alphabetic())
                    && arg.contains(if program == "fd" {
                        &['H', 'u'][..]
                    } else {
                        &['H', 'u', 'a'][..]
                    });
            skip = takes_value(program, arg);
            continue;
        }
        if !pattern {
            pattern = true;
            continue;
        }
        paths.push(arg.with_text(at(arg, &base)));
    }
    (paths, base, child_at, hidden)
}

fn takes_value(program: &str, arg: &Word) -> bool {
    if program == "fd" {
        [
            "-d",
            "-E",
            "-e",
            "-t",
            "-j",
            "--max-depth",
            "--exclude",
            "--extension",
            "--type",
            "--threads",
            "--changed-within",
            "--changed-before",
        ]
        .contains(&arg.as_str())
    } else if program == "du" {
        [
            "-B",
            "-I",
            "-t",
            "-d",
            "--block-size",
            "--max-depth",
            "--exclude",
        ]
        .contains(&arg.as_str())
            || !arg.starts_with("--") && arg.chars().last().is_some_and(|c| "dIBt".contains(c))
    } else {
        false
    }
}
