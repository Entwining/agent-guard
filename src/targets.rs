use crate::{CoverageGap, shell::CommandRecord};

pub use crate::record::Target;
use crate::record::{Direction, Effect, HostFacts, Via, Walk, Word};
#[derive(Debug, Default)]
pub struct Effects {
    pub targets: Vec<Target>,
    pub gaps: Vec<CoverageGap>,
    pub code: Vec<String>,
    pub inline: Vec<String>,
    pub dump: bool,
    pub variable: bool,
    pub hidden_content: bool,
    pub replace_advice: bool,
    pub hidden_listing: bool,
    pub consumes_listing: bool,
}

pub fn infer(command: &CommandRecord, cwd: &str, host: HostFacts<'_>) -> Effects {
    infer_at(command, cwd, host, 0)
}

fn infer_at(command: &CommandRecord, cwd: &str, host: HostFacts<'_>, depth: usize) -> Effects {
    let mut effects = Effects::default();
    if depth > crate::limits::MAX_NESTING {
        effects.gaps.push(CoverageGap::InspectionBudget);
        return effects;
    }
    for redirect in &command.redirects {
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
        target.glob = redirect.globs;
        target.expands = redirect.expands;
        effects.targets.push(target);
    }
    if command.function {
        return effects;
    }
    if command.program.is_none() {
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
    let program = ["python", "node", "ruby", "perl", "php", "lua"]
        .into_iter()
        .find(|base| {
            program.strip_prefix(base).is_some_and(|suffix| {
                !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit() || c == '.')
            })
        })
        .unwrap_or(program);
    let args = &command.argv[index + 1..];
    let read = |word: &Word, recursive| {
        Target::from_word(
            word,
            cwd,
            host,
            Effect::Read,
            if recursive { Walk::Visible } else { Walk::None },
        )
    };
    match program {
        "__observed_stream__" => effects.gaps.push(CoverageGap::UnresolvedTarget),
        "printf" | "echo" | "print" => {
            effects.variable = !(program != "echo" && args.first().is_some_and(|arg| arg == "-v"))
                && command.variables().any(|name| secret_name(name));
        }
        "true" | "false" | ":" | "unset" | "local" | "break" | "continue" | "return" => {}
        "read"
            if command.redirects.iter().any(|redirect| {
                matches!(
                    redirect.direction,
                    Direction::Heredoc | Direction::Herestring
                ) && !redirect.expands
            }) => {}
        "cd" | "pushd" | "popd" => {
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
        "setopt" | "unsetopt" | "emulate" => effects.gaps.push(CoverageGap::ExecutorDivergence),
        "set" => {
            effects.dump = command.shell && !command.argv[index].contains('/') && args.is_empty()
        }
        "typeset" | "declare" if command.shell && !command.argv[index].contains('/') => {
            effects.dump = args.is_empty()
                || args.len() == 1 && args[0].starts_with('-') && args[0].contains(['p', 'x']);
            effects.variable = args
                .iter()
                .any(|arg| !arg.starts_with('-') && !arg.contains('=') && secret_name(arg));
        }
        "cat" | "head" | "tail" | "less" | "more" | "bat" | "sort" | "uniq" | "cut" | "nl"
        | "base64" | "xxd" | "od" | "strings" => {
            for path in args
                .iter()
                .filter(|s| !s.starts_with('-') && s.as_str() != "__observed_stream__")
            {
                effects.targets.push(read(path, false));
            }
            if command.unresolved() {
                effects.gaps.push(CoverageGap::UnresolvedTarget);
            }
        }
        "ls" => {
            let mut recursive = false;
            let mut options = true;
            let mut paths = Vec::new();
            for arg in args {
                if options && arg == "--" {
                    options = false;
                } else if options && arg.starts_with('-') && arg.len() > 1 {
                    recursive |=
                        arg == "--recursive" || !arg.starts_with("--") && arg.contains('R');
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
                let mut target = read(path, recursive);
                target.effect = Effect::List;
                effects.targets.push(target);
            }
        }
        "rg" | "grep" | "ag" | "ack" => {
            infer_search(program, args, cwd, host, &mut effects);
            if command.unresolved() {
                effects.gaps.push(CoverageGap::UnresolvedTarget);
            }
        }
        "xargs" | "env" => infer_wrapper(program, args, command, cwd, host, &mut effects, depth),
        "fd" | "tree" | "du" | "find" => {
            infer_listing(program, args, command, cwd, host, &mut effects, depth)
        }
        "tar" => infer_tar(args, cwd, host, &mut effects),
        "git" => infer_git(args, cwd, host, &mut effects),
        "python" | "python3" | "node" | "bun" | "ruby" | "perl" | "php" | "osascript" | "lua"
        | "deno" => {
            effects.gaps.push(CoverageGap::InterpreterChosenRead);
            let (code, claimed) = interpreter_code(program, args);
            for code in code {
                if code.contains("json.load") {
                    effects.gaps.push(CoverageGap::UnresolvedTarget);
                }
                effects.inline.push(code);
            }
            for (index, arg) in args.iter().enumerate() {
                if let Some(path) = arg
                    .strip_prefix("--env-file=")
                    .or_else(|| arg.strip_prefix("--env-file-if-exists="))
                {
                    let target = Target::from_word(
                        &arg.with_text(path.to_owned()),
                        cwd,
                        host,
                        Effect::Use,
                        Walk::None,
                    );
                    effects.targets.push(target);
                }
                if !claimed.contains(&index) && !arg.starts_with('-') {
                    effects.targets.push(read(arg, false));
                }
            }
        }
        "bash" | "zsh" | "sh" | "dash" | "ksh" | "csh" | "tcsh" => {
            let mut claimed = Vec::new();
            if let Some(index) = args.iter().position(|s| {
                s.starts_with('-')
                    && s[1..].chars().all(|c| c.is_ascii_lowercase())
                    && s.contains('c')
            }) {
                if let Some(code) = args.get(index + 1) {
                    effects.code.push(code.text.clone());
                    claimed.push(index + 1);
                }
            } else if command.stdin != crate::record::Stdin::Shell {
                effects.gaps.push(CoverageGap::UnresolvedTarget);
            }
            for (index, arg) in args.iter().enumerate() {
                if !claimed.contains(&index) && !arg.starts_with('-') {
                    effects.targets.push(read(arg, false));
                }
            }
        }
        "eval" => effects
            .code
            .push(args.iter().map(Word::as_str).collect::<Vec<_>>().join(" ")),
        "printenv" => {
            effects.dump = args.is_empty();
            effects.variable = args.iter().any(|s| secret_name(s));
        }
        "export" if command.shell && !command.argv[index].contains('/') => {
            effects.dump =
                args.is_empty() || args.iter().any(|s| s.starts_with('-') && s.contains('p'))
        }
        _ => {
            effects.gaps.push(CoverageGap::UnknownProgram {
                program: program.to_owned(),
            });
            for arg in args.iter().filter(|s| !s.starts_with('-')) {
                effects.targets.push(read(arg, false));
            }
        }
    }
    effects
}

fn secret_name(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    ["TOKEN", "SECRET", "KEY", "PASSWORD", "CREDENTIAL"]
        .iter()
        .any(|part| name.contains(part))
}

fn child(command: &CommandRecord, argv: &[Word], cwd: &str) -> CommandRecord {
    CommandRecord {
        function: false,
        argv: argv.to_vec(),
        redirects: Vec::new(),
        pipeline: command.pipeline,
        cwd: cwd.to_owned(),
        nested: command.nested,
        program: (!argv.is_empty()).then_some(0),
        wrappers: command.wrappers.clone(),
        shell: command.shell,
        flags: command.flags.clone(),
        items: command.items.clone(),
        stdin: command.stdin.clone(),
    }
}

fn infer_wrapper(
    program: &str,
    args: &[Word],
    command: &CommandRecord,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
    depth: usize,
) {
    let mut index = 0;
    let mut cwd = cwd.to_owned();
    while let Some(arg) = args.get(index) {
        if program == "env" && arg == "-S" {
            if let Some(code) = args.get(index + 1) {
                effects.code.push(code.text.clone());
            }
            return;
        }
        if arg == "--" {
            index += 1;
            break;
        }
        if !arg.starts_with('-') && !(program == "env" && arg.contains('=')) {
            break;
        }
        let takes = if program == "env" {
            ["-u", "-P", "-C"].contains(&arg.as_str())
        } else {
            [
                "-a",
                "-d",
                "-E",
                "-I",
                "-L",
                "-n",
                "-P",
                "-s",
                "--arg-file",
                "--delimiter",
                "--replace",
                "--max-args",
            ]
            .contains(&arg.as_str())
        };
        if program == "env"
            && arg == "-C"
            && let Some(path) = args.get(index + 1)
        {
            cwd = at(path, &cwd);
        }
        if program == "xargs"
            && ["-a", "--arg-file"].contains(&arg.as_str())
            && let Some(path) = args.get(index + 1)
        {
            effects.targets.push(Target::from_word(
                path,
                &cwd,
                host,
                Effect::Read,
                Walk::None,
            ));
        }
        index += if takes { 2 } else { 1 };
    }
    if index >= args.len() {
        effects.dump = program == "env";
        return;
    }
    if program == "env" && args.iter().any(|arg| arg == "-i") {
        effects.dump = true;
    }
    let nested = child(command, &args[index..], &cwd);
    let mut result = infer_at(&nested, &cwd, host, depth + 1);
    if cwd != command.cwd {
        for target in &mut result.targets {
            target.path = at(&target.path, &cwd);
        }
    }
    effects.consumes_listing = program == "xargs"
        && args.get(index).is_some_and(|name| {
            [
                "cat", "head", "tail", "less", "more", "bat", "sed", "awk", "jq", "yq", "base64",
                "xxd", "od", "strings", "sort", "uniq", "cut", "nl", "sh", "bash", "zsh",
            ]
            .contains(&name.as_str())
        });
    effects.targets.extend(result.targets);
    effects.gaps.extend(result.gaps);
    effects.code.extend(result.code);
    effects.inline.extend(result.inline);
    effects.dump |= result.dump;
    effects.variable |= result.variable;
    effects.hidden_listing |= result.hidden_listing;
    effects.hidden_content |= result.hidden_content;
    effects.replace_advice |= result.replace_advice;
}

fn at(path: &str, base: &str) -> String {
    if path.starts_with('/') || path.starts_with('~') {
        path.to_owned()
    } else {
        format!("{base}/{path}")
    }
}

fn infer_git(args: &[Word], cwd: &str, host: HostFacts<'_>, effects: &mut Effects) {
    let mut index = 0;
    let mut base = cwd.to_owned();
    while let Some(arg) = args.get(index).filter(|arg| arg.starts_with('-')) {
        let takes = [
            "-C",
            "-c",
            "--git-dir",
            "--work-tree",
            "--namespace",
            "--exec-path",
        ]
        .contains(&arg.as_str());
        let path = arg
            .strip_prefix("--work-tree=")
            .map(|text| arg.with_text(text.to_owned()))
            .or_else(|| {
                if ["-C", "--work-tree"].contains(&arg.as_str()) {
                    args.get(index + 1).cloned()
                } else {
                    None
                }
            });
        if let Some(path) = path {
            base = crate::filesystem::normalize(&path, &base, "");
            effects.targets.push(Target::from_word(
                &path.with_text(base.clone()),
                cwd,
                host,
                Effect::Enter,
                Walk::None,
            ));
        }
        index += if takes { 2 } else { 1 };
    }
    let Some(sub) = args.get(index) else {
        return;
    };
    index += 1;
    effects.variable = sub == "credential" && args.get(index).is_some_and(|arg| arg == "fill");
    let names="branch tag remote switch push fetch pull merge rebase cherry-pick revert reflog rev-parse describe bisect init clone submodule worktree config lfs sparse-checkout".split_whitespace().any(|name|name==sub.as_str());
    let metadata="add rm mv restore checkout reset stash check-ignore check-attr update-index ls-files status clean commit".split_whitespace().any(|name|name==sub.as_str());
    let pathspec = !names && sub != "grep";
    let add = |path: &str, word: &Word, effect: Effect, walk: Walk, effects: &mut Effects| {
        let glob = pathspec && path.contains(['*', '?', '[']);
        let mut target = Target::from_word(
            &word.with_text(crate::filesystem::normalize(path, &base, "")),
            cwd,
            host,
            effect,
            walk,
        );
        target.glob = glob;
        effects.targets.push(target);
        if effect == Effect::Read
            && let Some((_, path)) = path.split_once(':')
        {
            let mut target = Target::from_word(
                &word.with_text(crate::filesystem::normalize(path, &base, "")),
                cwd,
                host,
                effect,
                Walk::None,
            );
            target.glob = glob;
            effects.targets.push(target);
        }
    };
    let keys: &[&str] = match sub.as_str() {
        "config" => &["-f", "--file", "--blob"],
        "commit" => &["-F", "--file", "--pathspec-from-file"],
        "tag" | "merge" => &["-F", "--file"],
        "add" | "rm" | "restore" | "reset" | "checkout" | "stash" => &["--pathspec-from-file"],
        _ => &[],
    };
    let mut pattern = sub != "grep";
    while index < args.len() {
        let arg = &args[index];
        let mut option_path = None;
        for key in keys {
            if arg == key {
                index += 1;
                option_path = args.get(index).cloned();
                break;
            }
            if let Some(path) = arg.strip_prefix(&format!("{key}=")) {
                option_path = Some(arg.with_text(path.to_owned()));
                break;
            }
            if !key.starts_with("--")
                && let Some(path) = arg.strip_prefix(key).filter(|path| !path.is_empty())
            {
                option_path = Some(arg.with_text(path.to_owned()));
                break;
            }
        }
        if let Some(path) = option_path {
            add(&path, &path, Effect::Read, Walk::None, effects);
        } else if !arg.starts_with('-') {
            if !pattern {
                pattern = true;
            } else {
                add(
                    arg,
                    arg,
                    if metadata {
                        Effect::Meta
                    } else if names {
                        Effect::Name
                    } else {
                        Effect::Read
                    },
                    if !names && !metadata {
                        Walk::Visible
                    } else {
                        Walk::None
                    },
                    effects,
                );
            }
        }
        index += 1;
    }
}

fn interpreter_code(program: &str, args: &[Word]) -> (Vec<String>, Vec<usize>) {
    let (code, value, glued) = match program {
        "python" | "python3" => ("c", "WX", true),
        "node" => ("ep", "", false),
        "bun" => ("ep", "", true),
        "ruby" => ("e", "rICEix", true),
        "perl" => ("eE", "MmIidDCFx", true),
        "php" => ("rR", "dcfz", true),
        "osascript" => ("e", "", true),
        "lua" => ("e", "l", true),
        _ => ("", "", false),
    };
    let mut found = Vec::new();
    let mut claimed = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if program == "deno" && arg == "eval" {
            claimed.push(index);
            for (offset, source) in args[index + 1..].iter().enumerate() {
                if !source.starts_with('-') {
                    found.push(source.text.clone());
                    claimed.push(index + 1 + offset);
                }
            }
            break;
        }
        let long = arg
            .strip_prefix("--")
            .map(|s| s.split_once('=').map_or((s, None), |(k, v)| (k, Some(v))));
        if let Some((key, inline)) = long
            && (["eval", "print"].contains(&key) || program == "php" && key == "run")
        {
            if let Some(source) = inline {
                found.push(source.to_owned());
            } else {
                index += 1;
                if let Some(source) = args.get(index) {
                    found.push(source.text.clone());
                    claimed.push(index);
                }
            }
        } else if arg.starts_with('-') && !arg.starts_with("--") {
            for (offset, ch) in arg.char_indices().skip(1) {
                if value.contains(ch) {
                    if offset + 1 == arg.len() {
                        index += 1;
                        claimed.push(index);
                    }
                    break;
                }
                if code.contains(ch) {
                    if offset + 1 < arg.len() && !glued {
                        continue;
                    }
                    if offset + 1 < arg.len() {
                        found.push(arg[offset + 1..].into());
                    } else {
                        index += 1;
                        if let Some(source) = args.get(index) {
                            found.push(source.text.clone());
                            claimed.push(index);
                        }
                    }
                    break;
                }
            }
        }
        index += 1;
    }
    (found, claimed)
}

fn infer_listing(
    program: &str,
    args: &[Word],
    command: &CommandRecord,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
    depth: usize,
) {
    let mut paths = Vec::new();
    let mut base = cwd.to_owned();
    let mut child_at = None;
    let mut skip = false;
    let mut pattern = program != "fd";
    for (index, arg) in args.iter().enumerate() {
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
            && !["-H", "-L", "-P"].contains(&arg.as_str())
        {
            break;
        }
        if arg.starts_with('-') {
            skip = if program == "fd" {
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
                    "--exclude-from",
                    "--files0-from",
                ]
                .contains(&arg.as_str())
                    || !arg.starts_with("--")
                        && arg.chars().last().is_some_and(|c| "dIBt".contains(c))
            } else {
                false
            };
            continue;
        }
        if !pattern {
            pattern = true;
            continue;
        }
        paths.push(arg.with_text(at(arg, &base)));
    }
    if paths.is_empty() {
        paths.push(Word::literal(base.clone()));
    }
    for path in paths {
        effects.targets.push(Target::from_word(
            &path,
            cwd,
            host,
            Effect::List,
            Walk::Visible,
        ));
    }
    effects.hidden_listing = program == "find"
        || args.iter().any(|arg| {
            arg == "--hidden"
                || arg == "--unrestricted"
                || arg.starts_with('-') && arg.contains(['H', 'u', 'a'])
        });
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
        let result = infer_at(
            &child(command, &args[start..end], &base),
            &base,
            host,
            depth + 1,
        );
        if effects.hidden_listing && args.get(start).is_some_and(|name| content_consumer(name)) {
            effects.hidden_content = true;
        }
        effects.targets.extend(result.targets);
        effects.gaps.extend(result.gaps);
        effects.code.extend(result.code);
        effects.inline.extend(result.inline);
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

fn infer_tar(args: &[Word], cwd: &str, host: HostFacts<'_>, effects: &mut Effects) {
    let mut archive = None;
    let mut create = false;
    let mut extract = false;
    let mut stdout = false;
    let mut operands = Vec::new();
    let mut base = cwd.to_owned();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        let cluster = !arg.starts_with("--") && (index == 0 || arg.starts_with('-'));
        if cluster {
            create |= arg.contains('c');
            extract |= arg.contains('x');
            stdout |= arg.contains('O');
        }
        create |= arg == "--create";
        extract |= ["--extract", "--get"].contains(&arg.as_str());
        stdout |= arg == "--to-stdout";
        if arg == "--file" || cluster && arg.ends_with('f') {
            index += 1;
            archive = args.get(index).cloned();
        } else if let Some(path) = arg.strip_prefix("--file=") {
            archive = Some(arg.with_text(path.to_owned()));
        } else if arg == "-C" || arg == "--directory" || arg == "--cd" {
            index += 1;
            if let Some(path) = args.get(index) {
                base = crate::filesystem::normalize(path, &base, "");
            }
        } else if !cluster && !arg.starts_with('-') {
            operands.push(arg.clone());
        }
        index += 1;
    }
    if let Some(path) = archive {
        effects.targets.push(Target::from_word(
            &path,
            cwd,
            host,
            if create { Effect::Write } else { Effect::Read },
            Walk::None,
        ));
    }
    for path in operands {
        effects.targets.push(Target::from_word(
            &path.with_text(if path.starts_with('~') {
                path.text.clone()
            } else {
                crate::filesystem::normalize(&path, &base, "")
            }),
            cwd,
            host,
            Effect::Read,
            Walk::Visible,
        ));
    }
    if extract && !stdout {
        effects
            .targets
            .push(Target::new(base, Effect::Write, Walk::None, Via::Operand));
    }
}

fn infer_search(
    program: &str,
    args: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    let mut operands = Vec::new();
    let mut globs = Vec::new();
    let mut explicit = false;
    let mut names = false;
    let mut hidden = false;
    let mut recursive = false;
    let mut no_hidden = false;
    let mut unrestricted = 0;
    let mut options = true;
    let mut position = 0;
    while position < args.len() {
        let arg = &args[position];
        if options && arg == "--" {
            options = false;
            position += 1;
            continue;
        }
        let mut value_option = None;
        if options && arg.starts_with("--") {
            let (key, value) = arg[2..]
                .split_once('=')
                .map_or((&arg[2..], None), |(k, v)| {
                    (k, Some(arg.with_text(v.to_owned())))
                });
            match key {
                "files" => names = true,
                "hidden" => {
                    hidden = true;
                    no_hidden = false;
                }
                "no-hidden" => {
                    hidden = false;
                    no_hidden = true;
                }
                "unrestricted" => {
                    unrestricted += 1;
                    hidden |= (program == "ag" || unrestricted >= 2) && !no_hidden;
                }
                "recursive" => {
                    recursive = program == "grep";
                    hidden |= recursive;
                }
                _ => {}
            }
            if (if program=="rg" {"regexp file glob iglob type type-not encoding replace color colors sort sortr max-depth max-filesize pre pre-glob engine threads max-columns type-add type-clear path-separator context-separator field-context-separator field-match-separator after-context before-context context max-count ignore-file dfa-size-limit regex-size-limit hyperlink-format"} else {"regexp file include exclude exclude-dir exclude-from label context after-context before-context max-count binary-files devices directories"}).split_whitespace().any(|option|option==key)
            {
                value_option = Some((key.to_owned(), value));
            }
        } else if options && arg.starts_with('-') && arg.len() > 1 {
            for (offset, ch) in arg.char_indices().skip(1) {
                if program == "rg" {
                    if ch == 'u' {
                        unrestricted += 1;
                        hidden |= unrestricted >= 2 && !no_hidden;
                    }
                    if ch == '.' {
                        hidden = true;
                    }
                    if ch == 'r' {
                        effects.replace_advice = true;
                    }
                } else if program == "grep" && (ch == 'r' || ch == 'R')
                    || program == "ag" && ch == 'u'
                {
                    hidden = true;
                    recursive |= program == "grep";
                }
                if (if program == "rg" {
                    "efgtTEABCmMjrd"
                } else {
                    "efABCmdD"
                })
                .contains(ch)
                {
                    let tail = &arg[offset + ch.len_utf8()..];
                    value_option = Some((
                        ch.to_string(),
                        if tail.is_empty() {
                            None
                        } else {
                            Some(arg.with_text(tail.trim_start_matches('=').to_owned()))
                        },
                    ));
                    break;
                }
            }
        } else {
            operands.push(arg.clone());
        }
        if let Some((key, inline)) = value_option {
            let value = inline.or_else(|| {
                position += 1;
                args.get(position).cloned()
            });
            if let Some(value) = value {
                match key.as_str() {
                    "d" | "directories" if program == "grep" && value == "recurse" => {
                        hidden = true;
                        recursive = true;
                    }
                    "e" | "regexp" => {
                        explicit = true;
                    }
                    "f" | "file" => {
                        explicit = true;
                        effects.targets.push(Target::from_word(
                            &value,
                            cwd,
                            host,
                            Effect::Read,
                            Walk::None,
                        ));
                    }
                    "g" | "glob" | "iglob" | "include" => globs.push(value.text),
                    "ignore-file" | "exclude-from" => effects.targets.push(Target::from_word(
                        &value,
                        cwd,
                        host,
                        Effect::Read,
                        Walk::None,
                    )),
                    _ => {}
                }
            }
        }
        position += 1;
    }
    if !explicit && !names && !operands.is_empty() {
        operands.remove(0);
    }
    let implicit = operands.is_empty() && (program != "grep" || hidden);
    if implicit {
        operands.push(Word::literal(cwd.to_owned()));
    }
    effects.hidden_listing = names && hidden;
    effects.hidden_content = hidden && !names;
    let hidden_walk = recursive || matches!(program, "rg" | "ag") && hidden;
    for root in operands {
        let mut target = Target::from_word(
            &root,
            cwd,
            host,
            if names && !hidden_walk {
                Effect::List
            } else {
                Effect::Read
            },
            if program != "grep" || hidden {
                Walk::Visible
            } else {
                Walk::None
            },
        );
        target.search = implicit && !args.iter().any(|arg| arg == "--help" || arg == "-h");
        effects.targets.push(target);
        if !names {
            for glob in &globs {
                if !glob.starts_with('!') {
                    let mut target = Target::new(
                        crate::filesystem::absolute_input(
                            &format!("{}/{}", root.text, glob.rsplit('/').next().unwrap_or(glob)),
                            cwd,
                            host.home,
                        ),
                        Effect::Read,
                        Walk::None,
                        Via::Operand,
                    );
                    target.glob = true;
                    effects.targets.push(target);
                }
            }
        }
    }
}

// This is the CodeFile token boundary, not language-specific syntax interpretation.
pub fn code_paths(code: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut cursor = 0;
    let mut quote = 0;
    while cursor < code.len() {
        let ch = code[cursor..].chars().next().unwrap_or_default();
        if ch.is_alphanumeric() || matches!(ch, '.' | '/' | '~' | '_' | '$') {
            let start = cursor;
            cursor += ch.len_utf8();
            while cursor < code.len() {
                let next = code[cursor..].chars().next().unwrap_or_default();
                if !(next.is_alphanumeric() || matches!(next, '.' | '/' | '~' | '_' | '-' | '$')) {
                    break;
                }
                cursor += next.len_utf8();
            }
            let token = &code[start..cursor];
            if (start == 0 || code.as_bytes()[start - 1] != b'\\')
                && (token.contains('/')
                    || token.starts_with('.')
                    || quote != 0
                    || matches!(
                        code.as_bytes().get(start.wrapping_sub(1)),
                        Some(b'\'' | b'"' | b'`')
                    )
                    || matches!(code.as_bytes().get(cursor), Some(b'\'' | b'"' | b'`')))
            {
                paths.push(token.to_owned());
            }
        } else {
            if matches!(ch, '\'' | '"' | '`')
                && (cursor == 0 || code.as_bytes()[cursor - 1] != b'\\')
            {
                if quote == 0 {
                    quote = ch as u8;
                } else if quote == ch as u8 {
                    quote = 0;
                }
            }
            cursor += ch.len_utf8();
        }
    }
    paths
}

#[cfg(test)]
mod record_tests {
    use super::*;
    #[test]
    fn control_flow_builtins_are_not_unmodelled_programs() {
        let host = HostFacts {
            home: "/h",
            user: None,
        };
        for source in ["break", "continue", "return", "D=public true"] {
            let observation =
                crate::shell::observe(source, crate::shell::Arm::Brush, host.home, "/p", true)
                    .unwrap();
            let command = observation
                .script
                .commands
                .iter()
                .find(|c| c.program.is_some())
                .unwrap();
            let effects = infer(command, &command.cwd, host);
            assert!(effects.targets.is_empty(), "{source}: {effects:?}");
            assert!(effects.gaps.is_empty(), "{source}: {effects:?}");
        }
    }
    #[test]
    fn compound_use_records_have_the_use_effect() {
        let packet: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/rust-m2-scopes.json")).unwrap();
        let host = HostFacts {
            home: "/h",
            user: None,
        };
        for row in packet["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["partition"] == "use")
        {
            let observation = crate::shell::observe(
                row["source"].as_str().unwrap(),
                crate::shell::Arm::Brush,
                host.home,
                "/h/p",
                true,
            )
            .unwrap();
            let command = observation
                .script
                .commands
                .iter()
                .find(|command| command.program.is_none() && !command.argv.is_empty())
                .unwrap();
            let effects = infer(command, &command.cwd, host);
            assert_eq!(effects.targets.len(), 1);
            assert_eq!(effects.targets[0].effect, Effect::Use);
            assert_eq!(effects.targets[0].unresolved, command.argv[0].text);
        }
        let row = packet["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "assignment-data-control")
            .unwrap();
        let observation = crate::shell::observe(
            row["source"].as_str().unwrap(),
            crate::shell::Arm::Brush,
            host.home,
            "/h/p",
            true,
        )
        .unwrap();
        let assignment = observation
            .script
            .commands
            .iter()
            .find(|command| command.program.is_none() && !command.argv.is_empty())
            .unwrap();
        assert!(infer(assignment, &assignment.cwd, host).targets.is_empty());
    }
    fn targets(source: &str) -> Vec<Target> {
        let host = HostFacts {
            home: "/h",
            user: Some("fixture-user"),
        };
        let script = crate::shell::observe_with_user(
            source,
            crate::shell::Arm::Brush,
            host.home,
            "/p",
            host.user,
            false,
        )
        .unwrap()
        .script;
        infer(&script.commands[0], "/p", host).targets
    }
    #[test]
    fn ls_records_list_effect() {
        for source in ["ls .env", "ls -R public"] {
            assert!(
                targets(source)
                    .iter()
                    .all(|target| target.effect == Effect::List)
            );
        }
    }
    #[test]
    fn search_flag_has_an_explicit_owner() {
        assert!(targets("rg needle")[0].search);
        assert!(!targets("rg needle public")[0].search);
        assert!(!targets("rg --help")[0].search);
        assert!(!targets("git log -p public")[0].search);
    }

    #[test]
    fn p7_role_dependencies_follow_go_owners() {
        let packet: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/rust-m2-filesystem.json"))
                .unwrap();
        for row in packet["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["role"].is_string())
        {
            let target = targets(row["source"].as_str().unwrap())
                .into_iter()
                .find(|t| t.path.starts_with("/h/Library/Containers"))
                .unwrap();
            assert_eq!(
                format!("{:?}", target.effect),
                row["role"].as_str().unwrap(),
                "{}: {}",
                row["id"],
                row["owner"]
            );
        }
    }
}
