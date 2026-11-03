use crate::options::{Binding, Grammar};
use crate::record::{Effect, HostFacts, OptionValue, Role, Word};

mod grammars;
mod launchers;

pub(super) struct Resolution {
    pub program: Option<usize>,
    pub wrappers: Vec<String>,
    pub shell: bool,
    pub cwd: String,
    pub source: Option<String>,
    pub gap: Option<crate::CoverageGap>,
    pub environment: Vec<EnvironmentChange>,
    pub wrapper_files: Vec<OptionValue>,
    pub unproven: Option<usize>,
}

pub(crate) enum EnvironmentChange {
    Clear,
    Unset(String),
    Set(String, Box<Word>),
}

impl EnvironmentChange {
    pub(crate) fn assignment(word: &Word) -> Option<Self> {
        let (name, _) = word.text.split_once('=')?;
        let mut value = word.suffix(name.len() + 1);
        if super::lexer::initial_quote(&value.raw) == super::lexer::Quote::Unquoted {
            value.raw = value
                .raw
                .split_once('=')
                .map_or(value.raw.clone(), |(_, raw)| raw.into());
        }
        Some(Self::Set(name.into(), Box::new(value)))
    }
}

fn text(argv: &[Word], index: usize) -> &str {
    argv.get(index).map_or("", Word::as_str)
}

/// Records a word the shell may turn into an option at a position where the
/// wrapper grammar reads options, so its representative proves no role.
fn check(result: &mut Resolution, argv: &[Word], index: usize) {
    if argv.get(index).is_some_and(Word::option_unknown) {
        result.unproven.get_or_insert(index);
    }
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
        cwd: String::new(),
        source: None,
        gap: None,
        environment: Vec::new(),
        wrapper_files: Vec::new(),
        unproven: None,
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
        if argv[index].exact().is_none() {
            result.unproven.get_or_insert(index);
        }
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
        let start = index;
        if !resolve_wrapper(&name, argv, &mut index, &mut result) {
            break;
        }
        if argv[start].exact().is_none() {
            result.unproven.get_or_insert(start);
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
    if argv.iter().take(index.saturating_add(1)).any(|word| {
        word.cardinality_unknown
            && !(matches!(word.role, Role::Assign | Role::Precommand)
                && super::statements::assignment(&word.text).is_some())
    }) {
        result.gap = Some(crate::CoverageGap::UnsupportedShellSyntax);
    }
    result.cwd = crate::record::entered(cwd, argv, &result.wrapper_files, host.home);
    result
}

pub(super) fn stdin_kind(command: &crate::record::Command) -> crate::record::Stdin {
    use crate::record::Stdin;
    let Some(index) = command.program else {
        return Stdin::None;
    };
    let name = command.argv[index].rsplit('/').next().unwrap_or("");
    if crate::targets::shell_invocation(name, &command.argv[index + 1..]).is_some_and(
        |invocation| !matches!(invocation.source, crate::targets::ShellSource::Code(_)),
    ) {
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
        "tsx",
    ]
    .contains(&name)
    {
        return Stdin::Code;
    }
    Stdin::None
}

fn resolve_wrapper(
    name: &str,
    argv: &mut [Word],
    index: &mut usize,
    result: &mut Resolution,
) -> bool {
    match name {
        "command" if text(argv, *index) == "command" => {
            mark(argv, index);
            check(result, argv, *index);
            while ["-p", "--"].contains(&text(argv, *index)) {
                *index += 1;
                check(result, argv, *index);
            }
            if ["-v", "-V"].contains(&text(argv, *index)) {
                *index = argv.len();
            }
        }
        // Assignments after the boundary are executable names, unless the
        // wrapper itself accepts assignments (env, sudo and doas).
        "exec" if result.shell && text(argv, *index) == "exec" => {
            mark(argv, index);
            check(result, argv, *index);
            while ["-c", "-l"].contains(&text(argv, *index)) {
                if text(argv, *index) == "-c" {
                    result.environment.push(EnvironmentChange::Clear);
                }
                mark(argv, index);
                check(result, argv, *index);
            }
            if text(argv, *index) == "-a" {
                mark(argv, index);
                mark(argv, index);
            }
        }
        "nohup" => {
            mark(argv, index);
            check(result, argv, *index);
            if text(argv, *index) == "--" {
                mark(argv, index);
            }
        }
        "timeout" => {
            mark(argv, index);
            options(result, argv, index, &grammars::TIMEOUT);
            // The duration.
            mark(argv, index);
        }
        "nice" => resolve_nice(result, argv, index),
        "script" | "arch" | "stdbuf" | "caffeinate" | "time" => {
            resolve_options(result, name, argv, index)
        }
        "sudo" | "doas" => resolve_privilege(result, argv, index),
        // An option makes envchain manage its store instead of running a
        // command.
        "envchain" if !text(argv, *index + 1).starts_with('-') => {
            mark(argv, index);
            check(result, argv, *index);
            if let Some(word) = argv.get_mut(*index) {
                word.role = Role::Namespace;
            }
            *index += 1;
        }
        "env" => resolve_environment(result, argv, index),
        "xargs" => resolve_xargs(result, argv, index),
        _ => return launchers::resolve(result, name, argv, index),
    }
    true
}

/// Binds a wrapper's options up to its first operand, marking each word
/// they span; `--` ends them and is marked too. The values that name a file
/// or directory become wrapper files.
fn options(
    result: &mut Resolution,
    argv: &mut [Word],
    index: &mut usize,
    grammar: &Grammar,
) -> Binding {
    let mut binding = Binding::default();
    while *index < argv.len() {
        if argv[*index].exact() == Some("--") {
            mark(argv, index);
            break;
        }
        let Some(words) = binding.option(argv, *index, grammar) else {
            break;
        };
        for _ in 0..words {
            mark(argv, index);
        }
    }
    if let Some(start) = binding.unproven {
        result.unproven.get_or_insert(start);
    }
    result.wrapper_files.extend(
        binding
            .values
            .iter()
            .map(|bound| bound.value.clone())
            .filter(|value| value.effect != Effect::Name),
    );
    binding
}

fn resolve_options(result: &mut Resolution, name: &str, argv: &mut [Word], index: &mut usize) {
    mark(argv, index);
    let binding = options(
        result,
        argv,
        index,
        match name {
            "script" => &grammars::SCRIPT,
            "arch" => &grammars::ARCH,
            "stdbuf" => &grammars::STDBUF,
            "caffeinate" => &grammars::CAFFEINATE,
            _ => &grammars::TIME,
        },
    );
    // script records the session in its file operand, or plays it back.
    let playback = binding.has("-p") || binding.values.iter().any(|bound| bound.name == "-T");
    if name == "script" && *index < argv.len() {
        result.wrapper_files.push(OptionValue {
            index: *index,
            prefix: 0,
            effect: if playback {
                Effect::Read
            } else {
                Effect::Write
            },
        });
        mark(argv, index);
    }
}

fn resolve_privilege(result: &mut Resolution, argv: &mut [Word], index: &mut usize) {
    mark(argv, index);
    options(result, argv, index, &grammars::PRIVILEGE);
    while let Some(change) = argv
        .get(*index)
        .filter(|word| super::statements::assignment(word).is_some())
        .and_then(EnvironmentChange::assignment)
    {
        result.environment.push(change);
        mark(argv, index);
    }
}

fn resolve_environment(result: &mut Resolution, argv: &mut [Word], index: &mut usize) {
    mark(argv, index);
    let binding = options(result, argv, index, &grammars::ENV);
    // A lone `-` also clears the environment.
    if binding.has("-i") || binding.has("--ignore-environment") || text(argv, *index) == "-" {
        result.environment.push(EnvironmentChange::Clear);
    }
    if text(argv, *index) == "-" {
        mark(argv, index);
    }
    for bound in binding.values {
        let value = bound.value.word(argv);
        match bound.name {
            "-u" | "--unset" => result
                .environment
                .push(EnvironmentChange::Unset(value.text)),
            "-S" | "--split-string" => {
                result.source = Some(value.text);
                result.wrappers.push("env-S".into());
                *index = argv.len();
                return;
            }
            _ => {}
        }
    }
    while let Some(change) = argv.get(*index).and_then(EnvironmentChange::assignment) {
        result.environment.push(change);
        mark(argv, index);
    }
}

fn resolve_xargs(result: &mut Resolution, argv: &mut [Word], index: &mut usize) {
    mark(argv, index);
    options(result, argv, index, &grammars::XARGS);
}

fn resolve_nice(result: &mut Resolution, argv: &mut [Word], index: &mut usize) {
    mark(argv, index);
    check(result, argv, *index);
    let option = text(argv, *index);
    if ["-n", "--adjustment"].contains(&option) {
        mark(argv, index);
        mark(argv, index);
    } else if option.starts_with("-n")
        || option.starts_with("--adjustment=")
        || option
            .strip_prefix('-')
            .is_some_and(|s| s.starts_with(|c: char| c.is_ascii_digit()))
    {
        mark(argv, index);
    }
}
