use crate::record::{HostFacts, Role, Word};

pub(super) struct Resolution {
    pub program: Option<usize>,
    pub wrappers: Vec<String>,
    pub shell: bool,
    pub cwd: String,
    pub source: Option<String>,
    pub gap: Option<crate::CoverageGap>,
    pub environment: Vec<EnvironmentChange>,
}

pub(super) enum EnvironmentChange {
    Clear,
    Unset(String),
    Set(String, Word),
}

impl EnvironmentChange {
    fn assignment(word: &Word) -> Option<Self> {
        let (name, value) = word.text.split_once('=')?;
        let mut value = word.with_text(value.into());
        if super::lexer::initial_quote(&value.raw) == super::lexer::Quote::Unquoted {
            value.raw = value
                .raw
                .split_once('=')
                .map_or(value.raw.clone(), |(_, raw)| raw.into());
        }
        Some(Self::Set(name.into(), value))
    }
}

fn text(argv: &[Word], index: usize) -> &str {
    argv.get(index).map_or("", Word::as_str)
}

fn mark(argv: &mut [Word], index: &mut usize) {
    if let Some(word) = argv.get_mut(*index) {
        word.role = Role::Precommand;
    }
    *index += 1;
}

pub(super) fn resolve(argv: &mut [Word], cwd: &str, host: HostFacts<'_>) -> Resolution {
    let mut result = Resolution {
        program: None,
        wrappers: Vec::new(),
        shell: true,
        cwd: cwd.into(),
        source: None,
        gap: None,
        environment: Vec::new(),
    };
    let mut index = 0;
    while let Some(word) = argv.get(index) {
        if !matches!(word.role, Role::Assign | Role::Precommand)
            && super::statements::assignment(&word.raw).is_none()
            && word.raw != "nocorrect"
        {
            break;
        }
        mark(argv, &mut index);
    }
    let mut last = "".to_owned();
    while ["builtin", "-", "noglob"].contains(&text(argv, index)) {
        last = text(argv, index).to_owned();
        mark(argv, &mut index);
    }
    if last == "builtin"
        && ![
            "echo", "printf", "print", "export", "typeset", "declare", "set", "command", "eval",
            "source", ".",
        ]
        .contains(&text(argv, index))
    {
        result.gap = Some(crate::CoverageGap::UnknownProgram {
            program: text(argv, index).to_owned(),
        });
        index = argv.len();
    }
    while index < argv.len() {
        let name = text(argv, index)
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_owned();
        match name.as_str() {
            "command" if text(argv, index) == "command" => {
                mark(argv, &mut index);
                while ["-p", "--"].contains(&text(argv, index)) {
                    index += 1;
                }
                if ["-v", "-V"].contains(&text(argv, index)) {
                    index = argv.len();
                }
            }
            // The Go unwrapper owns these boundaries (native/shell/argv.go:66-147).
            // Assignments after the boundary are executable names, unless the
            // wrapper itself accepts assignments (env, sudo and doas).
            "exec" if result.shell && text(argv, index) == "exec" => {
                mark(argv, &mut index);
                while ["-c", "-l"].contains(&text(argv, index)) {
                    if text(argv, index) == "-c" {
                        result.environment.push(EnvironmentChange::Clear);
                    }
                    mark(argv, &mut index);
                }
                if text(argv, index) == "-a" {
                    mark(argv, &mut index);
                    mark(argv, &mut index);
                }
            }
            "nohup" => {
                mark(argv, &mut index);
                if text(argv, index) == "--" {
                    mark(argv, &mut index);
                }
            }
            "timeout" => {
                mark(argv, &mut index);
                while text(argv, index).starts_with('-') {
                    if ["-s", "--signal", "-k", "--kill-after"].contains(&text(argv, index)) {
                        mark(argv, &mut index);
                    }
                    mark(argv, &mut index);
                }
                mark(argv, &mut index);
            }
            "nice" => {
                mark(argv, &mut index);
                let option = text(argv, index);
                if ["-n", "--adjustment"].contains(&option) {
                    mark(argv, &mut index);
                    mark(argv, &mut index);
                } else if option.starts_with("-n")
                    || option.starts_with("--adjustment=")
                    || option
                        .strip_prefix('-')
                        .is_some_and(|s| s.starts_with(|c: char| c.is_ascii_digit()))
                {
                    mark(argv, &mut index);
                }
            }
            "script" | "arch" | "stdbuf" | "caffeinate" | "time" => {
                mark(argv, &mut index);
                while index < argv.len() {
                    let option = text(argv, index);
                    let accepted = match name.as_str() {
                        "stdbuf" => {
                            option.starts_with("-i")
                                || option.starts_with("-o")
                                || option.starts_with("-e")
                        }
                        "caffeinate" => {
                            ["-d", "-i", "-s", "-u", "-m", "-t", "-w"].contains(&option)
                        }
                        _ => option.starts_with('-'),
                    };
                    if !accepted {
                        break;
                    }
                    let takes = match name.as_str() {
                        "script" => ["-F", "-t"].contains(&option),
                        "arch" => ["-e", "-d", "-arch"].contains(&option),
                        "stdbuf" => ["-i", "-o", "-e"].contains(&option),
                        "caffeinate" => ["-t", "-w"].contains(&option),
                        _ => ["-f", "-o", "--format", "--output"].contains(&option),
                    };
                    if takes {
                        mark(argv, &mut index);
                    }
                    mark(argv, &mut index);
                }
                if name == "script" && index < argv.len() {
                    mark(argv, &mut index);
                }
            }
            "sudo" | "doas" => {
                mark(argv, &mut index);
                while index < argv.len()
                    && text(argv, index) != "--"
                    && (text(argv, index).starts_with('-')
                        || super::statements::assignment(text(argv, index)).is_some())
                {
                    if super::statements::assignment(text(argv, index)).is_some()
                        && let Some(change) = EnvironmentChange::assignment(&argv[index])
                    {
                        result.environment.push(change);
                    }
                    let option = text(argv, index);
                    let takes = [
                        "--user",
                        "--group",
                        "--host",
                        "--prompt",
                        "--chdir",
                        "--chroot",
                        "--role",
                        "--type",
                        "--other-user",
                        "--close-from",
                        "--command-timeout",
                    ]
                    .contains(&option)
                        || option.strip_prefix('-').is_some_and(|letters| {
                            !letters.is_empty()
                                && letters.chars().all(|c| c.is_ascii_alphabetic())
                                && letters.ends_with([
                                    'u', 'g', 'h', 'p', 'C', 'D', 'R', 'T', 'r', 't', 'U',
                                ])
                        });
                    if takes {
                        mark(argv, &mut index);
                    }
                    mark(argv, &mut index);
                }
                if text(argv, index) == "--" {
                    mark(argv, &mut index);
                }
            }
            "envchain" => {
                mark(argv, &mut index);
                if text(argv, index).starts_with('-') {
                    index = argv.len();
                }
                if let Some(word) = argv.get_mut(index) {
                    word.role = Role::Namespace;
                }
                index += 1;
            }
            "env" => {
                mark(argv, &mut index);
                while index < argv.len() {
                    match text(argv, index) {
                        "-C" => {
                            if let Some(word) = argv.get_mut(index + 1) {
                                result.cwd = crate::filesystem::absolute_input(
                                    &word.text,
                                    &result.cwd,
                                    host.home,
                                );
                                word.role = Role::Precommand;
                            }
                            index += 2;
                        }
                        "-i" | "--ignore-environment" => {
                            result.environment.push(EnvironmentChange::Clear);
                            index += 1;
                        }
                        "-u" | "--unset" => {
                            if let Some(word) = argv.get(index + 1) {
                                result
                                    .environment
                                    .push(EnvironmentChange::Unset(word.text.clone()));
                            }
                            index += 2;
                        }
                        "-P" => index += 2,
                        "-S" => {
                            result.source = argv.get(index + 1).map(|w| w.text.clone());
                            result.wrappers.push("env-S".into());
                            index = argv.len();
                        }
                        option if option.starts_with('-') => index += 1,
                        _ => {
                            let Some(change) = EnvironmentChange::assignment(&argv[index]) else {
                                break;
                            };
                            result.environment.push(change);
                            index += 1;
                        }
                    }
                }
            }
            "xargs" => {
                mark(argv, &mut index);
                while text(argv, index).starts_with('-') {
                    if [
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
                    .contains(&text(argv, index))
                    {
                        index += 1;
                    }
                    index += 1;
                }
            }
            _ => break,
        }
        result.wrappers.push(name);
        result.shell = false;
    }
    if index < argv.len() {
        result.program = Some(index);
        argv[index].role = Role::Program;
        if argv[index].text.starts_with('=') && argv[index].text.len() > 1 {
            argv[index].text.remove(0);
        }
    }
    result
}

pub(crate) fn shell_code_flag(word: &Word) -> bool {
    word.starts_with('-') && word[1..].chars().all(|c| c.is_ascii_lowercase()) && word.contains('c')
}

pub(super) fn stdin_kind(command: &crate::record::Command) -> crate::record::Stdin {
    use crate::record::Stdin;
    let Some(index) = command.program else {
        return Stdin::None;
    };
    let name = command.argv[index].rsplit('/').next().unwrap_or("");
    if ["sh", "bash", "zsh", "dash", "ksh", "csh", "tcsh"].contains(&name)
        && !command.argv[index + 1..].iter().any(shell_code_flag)
    {
        return Stdin::Shell;
    }
    let name = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    if [
        "python",
        "node",
        "bun",
        "ruby",
        "perl",
        "php",
        "osascript",
        "lua",
        "deno",
    ]
    .contains(&name)
    {
        return Stdin::Code;
    }
    Stdin::None
}
