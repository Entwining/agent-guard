use crate::{CoverageGap, shell::CommandRecord};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub path: String,
    pub write: bool,
    pub recursive: bool,
    pub name_only: bool,
}
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

pub fn infer(command: &CommandRecord, cwd: &str) -> Effects {
    infer_at(command, cwd, 0)
}

fn infer_at(command: &CommandRecord, cwd: &str, depth: usize) -> Effects {
    let mut effects = Effects::default();
    if depth > crate::limits::MAX_NESTING {
        effects.gaps.push(CoverageGap::InspectionBudget);
        return effects;
    }
    for (path, write) in &command.redirects {
        effects.targets.push(Target {
            path: path.clone(),
            write: *write,
            recursive: false,
            name_only: false,
        });
    }
    let Some(program) = command
        .argv
        .first()
        .map(|s| s.rsplit('/').next().unwrap_or(s))
    else {
        return effects;
    };
    let args = &command.argv[1..];
    let read = |path: &str, recursive| Target {
        path: path.to_owned(),
        write: false,
        recursive,
        name_only: false,
    };
    match program {
        "__observed_stream__" => effects.gaps.push(CoverageGap::UnresolvedTarget),
        "printf" | "echo" | "print" => {
            effects.variable = !(program != "echo" && args.first().is_some_and(|arg| arg == "-v"))
                && command.variables.iter().any(|name| secret_name(name));
        }
        "true" | "false" | ":" | "setopt" | "unsetopt" | "emulate" | "cd" | "unset" | "local" => {}
        "set" => effects.dump = args.is_empty(),
        "typeset" | "declare" => {
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
            if command.unresolved {
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
                    paths.push(arg.as_str());
                }
            }
            effects.hidden_listing = args
                .iter()
                .any(|s| s.starts_with('-') && s.contains(['a', 'A']));
            if paths.is_empty() {
                paths.push(cwd);
            }
            for path in paths {
                let mut target = read(path, recursive);
                target.name_only = true;
                effects.targets.push(target);
            }
        }
        "rg" | "grep" | "ag" | "ack" => {
            infer_search(program, args, cwd, &mut effects);
            if command.unresolved {
                effects.gaps.push(CoverageGap::UnresolvedTarget);
            }
        }
        "xargs" | "env" => infer_wrapper(program, args, command, cwd, &mut effects, depth),
        "fd" | "tree" | "du" | "find" => infer_listing(program, args, cwd, &mut effects),
        "tar" => infer_tar(args, cwd, &mut effects),
        "git" => infer_git(args, cwd, &mut effects),
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
                    effects.code.push(code.clone());
                    claimed.push(index + 1);
                }
            } else {
                effects.gaps.push(CoverageGap::UnresolvedTarget);
            }
            for (index, arg) in args.iter().enumerate() {
                if !claimed.contains(&index) && !arg.starts_with('-') {
                    effects.targets.push(read(arg, false));
                }
            }
        }
        "eval" => effects.code.push(args.join(" ")),
        "printenv" => {
            effects.dump = args.is_empty();
            effects.variable = args.iter().any(|s| secret_name(s));
        }
        "export" if command.argv[0] == "export" => {
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

fn child(command: &CommandRecord, argv: &[String], cwd: &str) -> CommandRecord {
    CommandRecord {
        argv: argv.to_vec(),
        redirects: Vec::new(),
        unresolved: command.unresolved,
        pipeline: command.pipeline,
        cwd: cwd.to_owned(),
        variables: command.variables.clone(),
    }
}

fn infer_wrapper(
    program: &str,
    args: &[String],
    command: &CommandRecord,
    cwd: &str,
    effects: &mut Effects,
    depth: usize,
) {
    let mut index = 0;
    let mut cwd = cwd.to_owned();
    while let Some(arg) = args.get(index) {
        if program == "env" && arg == "-S" {
            if let Some(code) = args.get(index + 1) {
                effects.code.push(code.clone());
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
            cwd = crate::filesystem::normalize(path, &cwd, "");
        }
        if program == "xargs"
            && ["-a", "--arg-file"].contains(&arg.as_str())
            && let Some(path) = args.get(index + 1)
        {
            effects.targets.push(Target {
                path: path.clone(),
                write: false,
                recursive: false,
                name_only: false,
            });
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
    let result = infer_at(&nested, &cwd, depth + 1);
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

fn infer_git(args: &[String], cwd: &str, effects: &mut Effects) {
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
            .map(str::to_owned)
            .or_else(|| {
                if ["-C", "--work-tree"].contains(&arg.as_str()) {
                    args.get(index + 1).cloned()
                } else {
                    None
                }
            });
        if let Some(path) = path {
            base = crate::filesystem::normalize(&path, &base, "");
            effects.targets.push(Target {
                path: base.clone(),
                write: false,
                recursive: false,
                name_only: true,
            });
        }
        index += if takes { 2 } else { 1 };
    }
    let Some(sub) = args.get(index) else {
        return;
    };
    index += 1;
    effects.variable = sub == "credential" && args.get(index).is_some_and(|arg| arg == "fill");
    let names="branch tag remote switch push fetch pull merge rebase cherry-pick revert reflog rev-parse describe bisect init clone submodule worktree config lfs sparse-checkout".split_whitespace().any(|name|name==sub);
    let metadata="add rm mv restore checkout reset stash check-ignore check-attr update-index ls-files status clean commit".split_whitespace().any(|name|name==sub);
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
                option_path = Some(path.into());
                break;
            }
            if !key.starts_with("--")
                && let Some(path) = arg.strip_prefix(key).filter(|path| !path.is_empty())
            {
                option_path = Some(path.into());
                break;
            }
        }
        if let Some(path) = option_path {
            effects.targets.push(Target {
                path: crate::filesystem::normalize(&path, &base, ""),
                write: false,
                recursive: false,
                name_only: false,
            });
        } else if !arg.starts_with('-') {
            if !pattern {
                pattern = true;
            } else {
                effects.targets.push(Target {
                    path: crate::filesystem::normalize(arg, &base, ""),
                    write: false,
                    recursive: !names && !metadata,
                    name_only: names || metadata,
                });
                if !names
                    && !metadata
                    && let Some((_, path)) = arg.split_once(':')
                {
                    effects.targets.push(Target {
                        path: crate::filesystem::normalize(path, &base, ""),
                        write: false,
                        recursive: false,
                        name_only: false,
                    });
                }
            }
        }
        index += 1;
    }
}

fn interpreter_code(program: &str, args: &[String]) -> (Vec<String>, Vec<usize>) {
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
                    found.push(source.clone());
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
                    found.push(source.clone());
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
                            found.push(source.clone());
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

fn infer_listing(program: &str, args: &[String], cwd: &str, effects: &mut Effects) {
    let mut paths = Vec::new();
    let mut skip = false;
    let mut pattern = program != "fd";
    for arg in args {
        if skip {
            skip = false;
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
                    "-d",
                    "--block-size",
                    "--max-depth",
                    "--exclude",
                    "--exclude-from",
                    "--files0-from",
                ]
                .contains(&arg.as_str())
            } else {
                false
            };
            continue;
        }
        if !pattern {
            pattern = true;
            continue;
        }
        paths.push(arg.clone());
    }
    if paths.is_empty() {
        paths.push(cwd.to_owned());
    }
    for path in paths {
        effects.targets.push(Target {
            path,
            write: false,
            recursive: true,
            name_only: true,
        });
    }
    effects.hidden_listing = program == "find"
        || args.iter().any(|arg| {
            arg == "--hidden"
                || arg == "--unrestricted"
                || arg.starts_with('-') && arg.contains(['H', 'u', 'a'])
        });
}

fn infer_tar(args: &[String], cwd: &str, effects: &mut Effects) {
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
            archive = Some(path.into());
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
        effects.targets.push(Target {
            path,
            write: create,
            recursive: false,
            name_only: false,
        });
    }
    for path in operands {
        effects.targets.push(Target {
            path: if path.starts_with('~') {
                path
            } else {
                crate::filesystem::normalize(&path, &base, "")
            },
            write: false,
            recursive: true,
            name_only: false,
        });
    }
    if extract && !stdout {
        effects.targets.push(Target {
            path: base,
            write: true,
            recursive: false,
            name_only: false,
        });
    }
}

fn infer_search(program: &str, args: &[String], cwd: &str, effects: &mut Effects) {
    let mut operands = Vec::new();
    let mut patterns = Vec::new();
    let mut globs = Vec::new();
    let mut explicit = false;
    let mut names = false;
    let mut hidden = false;
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
                .map_or((&arg[2..], None), |(k, v)| (k, Some(v.to_owned())));
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
                    hidden |= unrestricted >= 2 && !no_hidden;
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
                } else if ch == 'r' || ch == 'R' {
                    hidden = true;
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
                            Some(tail.trim_start_matches('=').to_owned())
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
                    "e" | "regexp" => {
                        explicit = true;
                        patterns.push(value);
                    }
                    "f" | "file" => {
                        explicit = true;
                        effects.targets.push(Target {
                            path: value,
                            write: false,
                            recursive: false,
                            name_only: false,
                        });
                    }
                    "g" | "glob" | "iglob" | "include" => globs.push(value),
                    "ignore-file" | "exclude-from" => effects.targets.push(Target {
                        path: value,
                        write: false,
                        recursive: false,
                        name_only: false,
                    }),
                    _ => {}
                }
            }
        }
        position += 1;
    }
    if !explicit && !names && !operands.is_empty() {
        patterns.push(operands.remove(0));
    }
    if operands.is_empty() && (program != "grep" || hidden) {
        operands.push(cwd.to_owned());
    }
    effects.hidden_listing = names && hidden;
    effects.hidden_content = hidden && !names;
    for root in operands {
        effects.targets.push(Target {
            path: root.clone(),
            write: false,
            recursive: program != "grep" || hidden,
            name_only: names,
        });
        if !names {
            for glob in &globs {
                if !glob.starts_with('!') {
                    effects.targets.push(Target {
                        path: format!("{root}/{}", glob.rsplit('/').next().unwrap_or(glob)),
                        write: false,
                        recursive: false,
                        name_only: false,
                    });
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
