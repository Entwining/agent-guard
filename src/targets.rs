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
    pub search: Option<Search>,
}

#[derive(Debug)]
pub struct Search {
    pub pattern: String,
    pub glob: Option<String>,
}

pub fn infer(command: &CommandRecord, cwd: &str) -> Effects {
    let mut effects = Effects::default();
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
        "printf" | "echo" | "true" | "false" | ":" | "setopt" | "unsetopt" | "emulate" => {}
        "cat" => {
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
        "rg" | "grep" => {
            infer_search(program, args, cwd, &mut effects);
            if command.unresolved {
                effects.gaps.push(CoverageGap::UnresolvedTarget);
            }
        }
        "xargs" if args.first().is_some_and(|s| s == "cat") => {
            effects.consumes_listing = true;
            for path in args.iter().skip(1).filter(|s| !s.starts_with('-')) {
                effects.targets.push(read(path, false));
            }
        }
        "git" if args.first().is_some_and(|s| s == "commit") => {
            for (i, arg) in args.iter().enumerate() {
                if arg == "-F" || arg == "--file" {
                    if let Some(path) = args.get(i + 1) {
                        effects.targets.push(read(path, false));
                    }
                } else if let Some(path) = arg
                    .strip_prefix("-F")
                    .filter(|p| !p.is_empty())
                    .or_else(|| arg.strip_prefix("--file="))
                {
                    effects.targets.push(read(path, false));
                }
            }
        }
        "python" | "python3" | "node" => {
            effects.gaps.push(CoverageGap::InterpreterChosenRead);
            if let Some(index) = args
                .iter()
                .position(|s| s == "-c" || s == "-e" || s == "--eval")
            {
                if let Some(code) = args.get(index + 1) {
                    effects.inline.push(code.clone());
                    if code.contains("json.load") {
                        effects.gaps.push(CoverageGap::UnresolvedTarget);
                    }
                }
            } else {
                effects.gaps = vec![CoverageGap::UnresolvedTarget];
            }
        }
        "bash" | "zsh" | "sh" => {
            if let Some(index) = args.iter().position(|s| s == "-c") {
                if let Some(code) = args.get(index + 1) {
                    effects.code.push(code.clone());
                }
            } else {
                effects.gaps.push(CoverageGap::UnresolvedTarget);
            }
        }
        "eval" => effects.code.push(args.join(" ")),
        "env" if args.is_empty() || args.iter().any(|s| s == "-i") => effects.dump = true,
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
            if [
                "regexp",
                "file",
                "glob",
                "encoding",
                "replace",
                "ignore-file",
                "exclude-from",
            ]
            .contains(&key)
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
                if "efgEr".contains(ch) {
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
                    "g" | "glob" => globs.push(value),
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
    if operands.is_empty() {
        operands.push(cwd.to_owned());
    }
    effects.hidden_listing = names && hidden;
    effects.hidden_content = hidden && !names;
    for root in operands {
        effects.targets.push(Target {
            path: root.clone(),
            write: false,
            recursive: true,
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
    if let Some(pattern) = patterns.into_iter().next() {
        effects.search = Some(Search {
            pattern,
            glob: globs.into_iter().next(),
        });
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
