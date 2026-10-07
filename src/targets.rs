use crate::{CoverageGap, shell::CommandRecord};

pub use crate::record::Target;
use crate::record::{Direction, Effect, HostFacts, OptionRole, Role, Via, Walk, Word};
mod clients;
mod secrets;
const READERS: &str = "cat head tail less more bat sed awk jq yq base64 xxd od strings diff openssl plutil cp tee tar source . sort uniq cut nl fold rev paste comm join iconv hexdump hd zcat gzcat bzcat xzcat ag ack tac column pr vim vi nvim view perl ruby dd scp rsync zip ed ex hg svn sh bash zsh dash ksh wget php zgrep zless zmore";
const DATA_PROGRAMS: &str = "echo printf print : true false export set unset typeset declare local";

#[cfg(test)]
mod message_roles {
    #[test]
    fn commit_messages_do_not_become_pathspecs() {
        let host = crate::record::HostFacts {
            home: "/synthetic/home",
            user: None,
        };
        let observation = crate::shell::observe(
            "git commit -m 'public message'",
            crate::shell::Arm::Brush,
            host.home,
            "/synthetic/project",
            true,
        )
        .unwrap();
        let effects = super::infer(&observation.script.commands[0], "/synthetic/project", host);
        assert!(
            !effects
                .targets
                .iter()
                .any(|target| target.path.ends_with("public message"))
        );
    }
}

#[derive(Debug, Default)]
pub struct Effects {
    pub(crate) independent_arguments: bool,
    pub targets: Vec<Target>,
    pub gaps: Vec<CoverageGap>,
    pub code: Vec<String>,
    pub inline: Vec<String>,
    pub dump: bool,
    pub variable: bool,
    pub token: bool,
    pub keychain: bool,
    pub stored_secret: bool,
    pub trace: bool,
    pub hidden_content: bool,
    pub replace_advice: bool,
    pub include_advice: bool,
    pub bre_advice: bool,
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
        if redirect.stream.is_some() {
            continue;
        }
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
        target.glob = redirect.globs || redirect.shell_matches;
        target.glob_hidden = !redirect.globs && !redirect.shell_matches;
        target.expands = redirect.expands;
        target.runtime_unknown = redirect.runtime_unknown;
        effects.targets.push(target);
    }
    if command.function {
        if command.argv.iter().any(|word| word.field_count_unknown) {
            effects.gaps.push(CoverageGap::UnsupportedShellSyntax);
        }
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
    let cwd_program = program;
    let program = ["python", "node", "ruby", "perl", "php", "lua"]
        .into_iter()
        .find(|base| {
            program.strip_prefix(base).is_some_and(|suffix| {
                !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit() || c == '.')
            })
        })
        .unwrap_or(program);
    let args = &command.argv[index + 1..];
    let args = label_options(program, args, cwd, host, &mut effects);
    let args = args.as_ref();
    if !command.wrappers.iter().any(|wrapper| wrapper == "xargs") {
        for stream in list_file_sources(command) {
            if let crate::record::StreamOutput::Known(outputs) = stream {
                for output in outputs {
                    for path in output.lines().filter(|line| !line.is_empty()) {
                        effects.targets.push(Target::from_word(
                            &Word::literal(path.into()),
                            cwd,
                            host,
                            if program == "du" {
                                Effect::List
                            } else {
                                Effect::Read
                            },
                            Walk::None,
                        ));
                    }
                }
            } else {
                effects.gaps.push(CoverageGap::UnresolvedTarget);
            }
        }
    }
    if command.argv[index].contains('/') {
        let mut target =
            Target::from_word(&command.argv[index], cwd, host, Effect::Use, Walk::Visible);
        target.via = Via::Option;
        effects.targets.push(target);
    }
    if command.stdin == crate::record::Stdin::Code {
        effects.inline.extend(
            command
                .redirects
                .iter()
                .filter(|r| matches!(r.direction, Direction::Heredoc | Direction::Herestring))
                .map(|r| r.target.clone()),
        );
    }
    let hidden_items_read = if let Some(items) = &command.items {
        let effect = match program {
            "echo" | "printf" | "print" | ":" | "true" | "false" | "export" | "set" | "unset"
            | "typeset" | "declare" | "local" | "cd" | "curl" | "wget" | "docker" | "ssh" => {
                Effect::Name
            }
            "stat" | "test" | "[" | "chmod" | "chown" | "chgrp" | "chflags" | "touch" | "rm"
            | "rmdir" | "mkdir" | "mv" | "ln" | "wc" | "file" | "shasum" | "sha1sum"
            | "sha256sum" | "md5" | "md5sum" | "cksum" | "realpath" | "readlink" | "basename"
            | "dirname" => Effect::Meta,
            "ls" | "tree" | "du" | "find" | "fd" => Effect::List,
            "pushd" | "popd" => Effect::Enter,
            "tee" => Effect::Write,
            "ssh-add" => Effect::Use,
            _ => Effect::Read,
        };
        let walk = if items.hidden {
            Walk::Hidden
        } else {
            Walk::Visible
        };
        effects
            .targets
            .push(Target::new(items.root.clone(), effect, walk, Via::Items));
        effect == Effect::Read && walk == Walk::Hidden
    } else {
        false
    };
    if command.wrappers.iter().any(|w| w == "xargs") {
        effects.consumes_listing = xargs_content_consumer(program);
        let options = &command.argv[..index];
        for (i, word) in options.iter().enumerate() {
            let file = if word == "-a" || word == "--arg-file" {
                options.get(i + 1).cloned()
            } else {
                word.strip_prefix("--arg-file=")
                    .map(|path| word.with_text(path.into()))
            };
            if let Some(file) = file {
                let mut target = Target::from_word(&file, cwd, host, Effect::Read, Walk::None);
                target.via = Via::Option;
                effects.targets.push(target);
            }
        }
    }
    secrets::infer(program, args, &mut effects);
    let read = |word: &Word, recursive| {
        Target::from_word(
            word,
            cwd,
            host,
            if word.role == Role::Option(OptionRole::Name) {
                Effect::Name
            } else {
                Effect::Read
            },
            if recursive { Walk::Visible } else { Walk::None },
        )
    };
    let mut generic_walk = None;
    let mut claimed = Vec::new();
    match program {
        "__observed_stream__" => effects.gaps.push(CoverageGap::UnresolvedTarget),
        "printf" | "echo" | "print" => {
            effects.independent_arguments = args.iter().all(|arg| !arg.starts_with('-'));
            effects.targets.extend(
                args.iter()
                    .filter(|arg| arg.globs && !arg.starts_with('-'))
                    .map(|arg| Target::from_word(arg, cwd, host, Effect::Name, Walk::None)),
            );
            effects.variable = !(program != "echo" && args.first().is_some_and(|arg| arg == "-v"))
                && command.variables().any(|name| secret_name(name));
        }
        "true" | "false" | ":" | "unset" | "local" | "break" | "continue" | "return" | "shift" => {
            effects.independent_arguments = matches!(program, "true" | "false" | ":" | "local")
                && args.iter().any(|arg| arg.cardinality_unknown)
                && args.iter().all(|arg| !arg.starts_with('-'));
        }
        "tr" => {}
        "mktemp" => {
            effects.gaps.push(CoverageGap::UnknownProgram {
                program: program.into(),
            });
            effects.targets.extend(args.iter().filter_map(|word| {
                operand_value(word).map(|value| {
                    Target::from_word(
                        &word.with_text(value.into()),
                        cwd,
                        host,
                        Effect::Write,
                        Walk::None,
                    )
                })
            }));
        }
        "df" | "stat" | "test" | "[" | "chmod" | "chown" | "chgrp" | "chflags" | "touch"
        | "rmdir" | "mkdir" | "mv" | "ln" | "wc" | "file" | "shasum" | "sha1sum" | "sha256sum"
        | "md5" | "md5sum" | "cksum" | "realpath" | "readlink" | "basename" | "dirname" => {
            // native/targets/programs.go:42-48 assigns metadata, with no walk
            // for stat/test/[ /mkdir/mv and visible walk for the other entries.
            let walk = if ["df", "stat", "test", "[", "mkdir", "mv", "readlink"].contains(&program)
            {
                Walk::None
            } else {
                Walk::Visible
            };
            effects.targets.extend(args.iter().filter_map(|word| {
                operand_value(word).map(|value| {
                    Target::from_word(&word.with_text(value.into()), cwd, host, Effect::Meta, walk)
                })
            }));
        }
        "read"
            if command.pipeline.is_some()
                || command.redirects.iter().any(|redirect| {
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
        "setopt" | "unsetopt" | "emulate" => {
            effects.gaps.push(
                if command.shell && command.argv[..index].iter().all(|w| w.contains('=')) {
                    CoverageGap::ExecutorDivergence
                } else {
                    CoverageGap::UnknownProgram {
                        program: program.into(),
                    }
                },
            );
        }
        "set" => {
            effects.dump = command.shell && !command.argv[index].contains('/') && args.is_empty()
        }
        "typeset" | "declare" if command.shell && !command.argv[index].contains('/') => {
            effects.independent_arguments = args.iter().any(|arg| arg.cardinality_unknown)
                && args.iter().all(|arg| arg.contains('='));
            effects.dump = args.is_empty()
                || args.len() == 1 && args[0].starts_with('-') && args[0].contains(['p', 'x']);
            effects.variable = args
                .iter()
                .any(|arg| !arg.starts_with('-') && !arg.contains('=') && secret_name(arg));
        }
        "cat" | "head" | "tail" | "less" | "more" | "bat" | "sort" | "uniq" | "cut" | "nl"
        | "base64" | "xxd" | "od" | "strings" => {
            effects.independent_arguments = args.iter().any(|arg| arg.cardinality_unknown)
                && args.iter().all(|arg| !arg.starts_with('-'));
            generic_walk = Some(Walk::None);
            if command.unresolved() {
                effects.gaps.push(CoverageGap::UnresolvedTarget);
            }
        }
        "ls" => {
            effects.independent_arguments = args.iter().any(|arg| arg.cardinality_unknown)
                && args.iter().all(|arg| !arg.starts_with('-'));
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
        "tar" => {
            claimed = infer_tar(args, cwd, host, &mut effects);
            generic_walk = Some(Walk::Visible);
        }
        "jq" | "yq" => {
            // native/targets/programs.go:172-192 claims the filter, while
            // -f/--from-file leaves the filter-file operand readable.
            if !args.iter().any(|arg| arg == "-f" || arg == "--from-file") {
                let mut operands = args
                    .iter()
                    .enumerate()
                    .filter(|(_, arg)| !arg.starts_with('-'));
                let first = operands.next();
                let filter = if program == "yq"
                    && first.is_some_and(|(_, arg)| {
                        ["eval", "e", "eval-all", "ea"].contains(&arg.as_str())
                    }) {
                    operands.next()
                } else {
                    first
                };
                if let Some((index, _)) = filter {
                    claimed.push(index);
                }
            }
            generic_walk = Some(Walk::Visible);
        }
        "rm" => {
            // native/targets/programs.go:42-48 assigns metadata operands.
            effects.targets.extend(
                args.iter()
                    .filter(|word| !word.starts_with('-'))
                    .map(|word| Target::from_word(word, cwd, host, Effect::Meta, Walk::Visible)),
            );
        }
        "git" => {
            // D48 gives shell-supplied locations the Go option roles
            // (native/targets/git.go:26-48); Go does not yet infer these env values.
            for (name, value) in &command.environment {
                let effect = match name.as_str() {
                    "GIT_DIR" => Effect::Read,
                    "GIT_WORK_TREE" => Effect::Enter,
                    _ => continue,
                };
                if !value.text.is_empty() {
                    let mut target = Target::from_word(
                        value,
                        cwd,
                        host,
                        effect,
                        if args.iter().any(|arg| arg == "config") {
                            Walk::None
                        } else {
                            Walk::Visible
                        },
                    );
                    target.via = Via::Option;
                    effects.targets.push(target);
                }
            }
            infer_git(args, cwd, host, &mut effects);
        }
        "cp" | "install" | "rsync" | "tee" | "ssh-add" | "dotenvx" | "ssh" | "scp" | "sftp"
        | "ssh-keygen" | "dd" | "kubectl" | "npm" | "curl" | "wget" | "docker" => {
            effects
                .targets
                .extend(clients::infer(program, args, cwd, host));
        }
        "ctags" => {
            effects.gaps.push(CoverageGap::UnknownProgram {
                program: program.to_owned(),
            });
            effects
                .targets
                .extend(clients::infer(program, args, cwd, host));
        }
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
            // Without flags each argument has its own read role; no combination
            // can select code, consume another argument, or change its target.
            effects.independent_arguments = args.iter().all(|arg| !arg.starts_with('-'));
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
        // The shell evaluator already observes builtin eval in its binding/cwd
        // scope. Replaying it here both loses that scope and doubles nested work.
        "eval" if !command.shell => effects
            .code
            .push(args.iter().map(Word::as_str).collect::<Vec<_>>().join(" ")),
        "eval" => {}
        "printenv" => {
            effects.dump = args.is_empty();
            effects.variable = args.iter().any(|s| secret_name(s));
        }
        "export" if command.shell && !command.argv[index].contains('/') => {
            effects.independent_arguments = args.iter().any(|arg| arg.cardinality_unknown)
                && args.iter().all(|arg| arg.contains('='));
            effects.dump =
                args.is_empty() || args.iter().any(|s| s.starts_with('-') && s.contains('p'))
        }
        _ => {
            effects.gaps.push(CoverageGap::UnknownProgram {
                program: program.to_owned(),
            });
            generic_walk = Some(Walk::Visible);
        }
    }
    // Go's generic operand loop owns unclaimed glued values, including readers
    // (native/targets/infer.go:79-95,187-209); special adapters retain their roles.
    if let Some(walk) = generic_walk {
        for (index, arg) in args.iter().enumerate() {
            if !claimed.contains(&index)
                && arg.role != Role::Option(OptionRole::Name)
                && arg.as_str() != "__observed_stream__"
                && let Some(value) = if arg.role == Role::Path {
                    (!arg.value.is_empty()).then_some(arg.value.as_str())
                } else {
                    operand_value(arg)
                }
            {
                effects.targets.push(Target::from_word(
                    &arg.with_text(value.into()),
                    cwd,
                    host,
                    if !modelled_program(program)
                        && value.contains("://")
                        && !value
                            .get(..7)
                            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
                    {
                        Effect::Name
                    } else {
                        Effect::Read
                    },
                    walk,
                ));
            }
        }
    }
    effects.hidden_content |= hidden_items_read;
    // A repetition projects each literal, not every possible argument sequence.
    // Positional roles and executable text cannot use those projections as a
    // complete command. Independent operand owners above can check their union.
    if !effects.independent_arguments && args.iter().any(|word| word.cardinality_unknown)
        || command
            .environment
            .iter()
            .any(|(_, word)| word.cardinality_unknown)
    {
        effects.gaps.push(CoverageGap::UnsupportedShellSyntax);
    }
    effects.dump |= effects.inline.iter().any(|code| printenv_signature(code));
    let display = READERS
        .split_whitespace()
        .chain(["echo", "printf", "print"])
        .any(|name| name == program)
        && !(program != "echo"
            && ["printf", "print"].contains(&program)
            && args.first().is_some_and(|arg| arg == "-v"));
    effects.variable |= display
        && command
            .redirects
            .iter()
            .flat_map(|r| &r.vars)
            .any(|name| secret_name(name));
    // Go's command cwd owner (native/targets/infer.go:219-226) also covers
    // unknown programs; reader arguments alone do not model their behavior.
    let named = effects.targets.iter().any(|target| {
        matches!(target.via, Via::Operand | Via::Cwd | Via::Scan)
            && !matches!(target.effect, Effect::Enter | Effect::Name)
    });
    if !DATA_PROGRAMS
        .split_whitespace()
        .any(|name| name == cwd_program)
        && !["cd", "pushd", "popd"].contains(&cwd_program)
        && (!named || !modelled_program(cwd_program))
    {
        effects.targets.push(Target::new(
            cwd.to_owned(),
            Effect::Enter,
            Walk::None,
            Via::Cwd,
        ));
    }
    effects
}

fn label_options<'a>(
    program: &str,
    args: &'a [Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) -> std::borrow::Cow<'a, [Word]> {
    let mut labelled = std::borrow::Cow::Borrowed(args);
    let mut options = true;
    // Go resolves program options before GlobalOptions (infer.go:87-104).
    // Keep values in place so an adapter can claim a stronger program role.
    for (index, word) in args.iter().enumerate() {
        if word == "--" {
            options = false;
            continue;
        }
        if !options {
            labelled.to_mut()[index].role = Role::Path;
            continue;
        }
        let key = if word.value.starts_with('-') {
            word.value.split_once('=').map_or("", |(key, _)| key)
        } else {
            args.get(index.wrapping_sub(1)).map_or("", Word::as_str)
        };
        if clients::option(program, key).is_none()
            && ["--exclude", "--exclude-dir", "--include"].contains(&key)
        {
            let option_value = if word.value.starts_with('-') {
                word.value
                    .split_once('=')
                    .map_or(word.value.as_str(), |(_, value)| value)
            } else {
                &word.value
            };
            labelled.to_mut()[index].role = Role::Option(OptionRole::Name);
            let mut target = Target::from_word(
                &word.with_text(option_value.into()),
                cwd,
                host,
                Effect::Name,
                Walk::None,
            );
            target.via = Via::Option;
            effects.targets.push(target);
        }
    }
    labelled
}

fn modelled_program(program: &str) -> bool {
    // Keep the model boundary aligned with native/targets/programs.go Specs.
    READERS.split_whitespace().chain(DATA_PROGRAMS.split_whitespace()).chain(
        "stat test [ chmod chown chgrp chflags touch rm rmdir mkdir mv ln wc file shasum sha1sum sha256sum md5 md5sum cksum realpath readlink basename dirname cd pushd popd gh ls tree du install sftp curl git docker node bun deno kubectl ssh ssh-add ssh-keygen dotenvx npm pnpm yarn rg grep find fd".split_whitespace()
    ).any(|name| name == program)
}

fn printenv_signature(code: &str) -> bool {
    let boundary = |c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-';
    code.match_indices("printenv").any(|(start, name)| {
        code[..start].chars().next_back().is_none_or(boundary)
            && code[start + name.len()..]
                .chars()
                .next()
                .is_none_or(boundary)
    })
}

fn secret_name(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    ["TOKEN", "SECRET", "KEY", "PASSWORD", "CREDENTIAL"]
        .iter()
        .any(|part| name.contains(part))
}

pub(crate) fn shows_hidden(args: &[Word]) -> bool {
    args.iter().any(|w| {
        ["--hidden", "--unrestricted"].contains(&w.as_str())
            || w.starts_with('-')
                && !w.starts_with("--")
                && w[1..].chars().all(|c| c.is_ascii_alphabetic())
                && w.contains(['H', 'u'])
    })
}

fn child(command: &CommandRecord, argv: &[Word], cwd: &str) -> CommandRecord {
    CommandRecord {
        function: false,
        environment: command.environment.clone(),
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
        && args
            .get(index)
            .is_some_and(|name| xargs_content_consumer(name));
    effects.targets.extend(result.targets);
    effects.gaps.extend(result.gaps);
    effects.code.extend(result.code);
    effects.inline.extend(result.inline);
    effects.dump |= result.dump;
    effects.variable |= result.variable;
    effects.token |= result.token;
    effects.keychain |= result.keychain;
    effects.stored_secret |= result.stored_secret;
    effects.trace |= result.trace;
    effects.hidden_listing |= result.hidden_listing;
    effects.hidden_content |= result.hidden_content;
    effects.replace_advice |= result.replace_advice;
    effects.include_advice |= result.include_advice;
    effects.bre_advice |= result.bre_advice;
}

fn at(path: &str, base: &str) -> String {
    if path.starts_with('/') || path.starts_with('~') {
        path.to_owned()
    } else {
        format!("{base}/{path}")
    }
}

fn xargs_content_consumer(name: &str) -> bool {
    [
        "cat", "head", "tail", "less", "more", "bat", "sed", "awk", "jq", "yq", "base64", "xxd",
        "od", "strings", "sort", "uniq", "cut", "nl", "sh", "bash", "zsh",
    ]
    .contains(&name)
}

pub(crate) fn list_file_sources(command: &CommandRecord) -> Vec<&crate::record::StreamOutput> {
    let name = command
        .program
        .and_then(|index| command.argv.get(index))
        .map_or("", |word| word.rsplit('/').next().unwrap_or(word));
    let xargs = command.wrappers.iter().any(|wrapper| wrapper == "xargs");
    command
        .argv
        .iter()
        .enumerate()
        .filter_map(|(index, word)| {
            let stream = word.stream.as_deref()?;
            let option = word.split_once('=').map_or_else(
                || {
                    command
                        .argv
                        .get(index.wrapping_sub(1))
                        .map_or("", Word::as_str)
                },
                |(option, _)| option,
            );
            ((xargs && ["-a", "--arg-file"].contains(&option))
                || (name == "tar" && option == "-T")
                || (["tar", "rsync"].contains(&name) && option == "--files-from")
                || (["sort", "du"].contains(&name) && option == "--files0-from"))
                .then_some(stream)
        })
        .collect()
}

fn infer_git(args: &[Word], cwd: &str, host: HostFacts<'_>, effects: &mut Effects) {
    let mut index = 0;
    let mut base = cwd.to_owned();
    let option_walk = if args.iter().any(|arg| arg == "config") {
        Walk::None
    } else {
        Walk::Visible
    };
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
                option_walk,
            ));
        }
        // gitTargets leaves this value unclaimed (native/targets/git.go:26-48),
        // so commandTargets' operand fallback reads it at the command cwd.
        let directory = arg
            .strip_prefix("--git-dir=")
            .map(|text| arg.with_text(text.to_owned()))
            .or_else(|| {
                (arg == "--git-dir")
                    .then(|| args.get(index + 1).cloned())
                    .flatten()
            });
        if let Some(directory) = directory {
            effects.targets.push(Target::from_word(
                &directory,
                cwd,
                host,
                Effect::Read,
                option_walk,
            ));
        }
        if arg == "-c"
            && let Some(value) = args.get(index + 1)
        {
            // Go leaves -c's value for the Read operand fallback (git.go:26-48).
            effects.targets.push(Target::from_word(
                value,
                cwd,
                host,
                Effect::Read,
                option_walk,
            ));
        }
        index += if takes { 2 } else { 1 };
    }
    let Some(sub) = args.get(index) else {
        return;
    };
    index += 1;
    // native/rules/secrets.go:166-177 owns credential fill as SecretPrint.
    effects.stored_secret |=
        sub == "credential" && args.get(index).is_some_and(|arg| arg == "fill");
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
        "blame" => &["--contents"],
        "commit" => &["-F", "--file", "--pathspec-from-file"],
        "tag" | "merge" => &["-F", "--file"],
        "add" | "rm" | "restore" | "reset" | "checkout" | "stash" => &["--pathspec-from-file"],
        _ => &[],
    };
    let mut pattern = sub != "grep";
    let mut options = true;
    let mut bundle_action = None;
    let mut bundle_output = false;
    while index < args.len() {
        let arg = &args[index];
        if options && sub == "commit" && ["-m", "--message"].contains(&arg.as_str()) {
            index += 2;
            continue;
        }
        let mut option_path = None;
        if sub == "grep" && options {
            if arg == "--" {
                options = false;
                index += 1;
                continue;
            }
            if arg == "-e" {
                pattern = true;
                index += 2;
                continue;
            }
            if let Some(path) = arg
                .strip_prefix("--file=")
                .or_else(|| arg.strip_prefix("-f="))
            {
                option_path = Some(arg.with_text(path.to_owned()));
            } else if arg == "-f" {
                pattern = true;
                index += 1;
                option_path = args.get(index).cloned();
            }
        }
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
        } else if !options || !arg.starts_with('-') {
            // native/targets/git.go:109-151 claims the action and writes create's file.
            if sub == "bundle" && bundle_action.is_none() {
                bundle_action = Some(arg.text.clone());
                index += 1;
                continue;
            }
            if !pattern {
                pattern = true;
            } else {
                let output =
                    sub == "bundle" && bundle_action.as_deref() == Some("create") && !bundle_output;
                bundle_output |= output;
                add(
                    arg,
                    arg,
                    if output {
                        Effect::Write
                    } else if metadata {
                        Effect::Meta
                    } else if names {
                        Effect::Name
                    } else {
                        Effect::Read
                    },
                    if !output && !names && !metadata {
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
                // Perl's in-place suffix is attached only; a bare -i does not
                // consume the next word (native/shell/interpreters.go:75-77).
                if program == "perl" && ch == 'i' {
                    break;
                }
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

fn infer_tar(args: &[Word], cwd: &str, host: HostFacts<'_>, effects: &mut Effects) -> Vec<usize> {
    let mut claimed = Vec::new();
    let mut archive = None;
    let mut create = false;
    let mut extract = false;
    let mut stdout = false;
    let mut operands = Vec::new();
    let mut base = cwd.to_owned();
    let mut index = 0;
    let mut options = true;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" && options {
            options = false;
            index += 1;
            continue;
        }
        if !options {
            operands.push(arg.clone());
            claimed.push(index);
            index += 1;
            continue;
        }
        if arg.role == Role::Option(OptionRole::Name) {
            index += 1;
            continue;
        }
        let cluster = !arg.starts_with("--") && (index == 0 || arg.starts_with('-'));
        let flags = arg.strip_prefix('-').unwrap_or(arg.as_str());
        let short = cluster
            .then(|| flags.char_indices().find(|(_, c)| "fCTX".contains(*c)))
            .flatten();
        let letters = short.map_or(flags, |(at, c)| &flags[..at + c.len_utf8()]);
        if cluster {
            create |= letters.contains('c');
            extract |= letters.contains('x');
            stdout |= letters.contains('O');
        }
        create |= arg == "--create";
        extract |= ["--extract", "--get"].contains(&arg.as_str());
        stdout |= arg == "--to-stdout";
        if let Some((at, option)) = short {
            claimed.push(index);
            let tail = &flags[at + option.len_utf8()..];
            let value = if tail.is_empty() {
                index += 1;
                claimed.push(index);
                args.get(index).cloned()
            } else {
                Some(arg.with_text(tail.into()))
            };
            if let Some(value) = value {
                match option {
                    'f' => archive = Some(value),
                    'C' => base = crate::filesystem::normalize(&value, &base, ""),
                    _ => {
                        let mut target =
                            Target::from_word(&value, cwd, host, Effect::Read, Walk::Visible);
                        target.via = Via::Option;
                        effects.targets.push(target);
                    }
                }
            }
        } else if arg == "--file" {
            index += 1;
            archive = args.get(index).cloned();
            claimed.push(index);
        } else if let Some(path) = arg.strip_prefix("--file=") {
            archive = Some(arg.with_text(path.to_owned()));
            claimed.push(index);
        } else if let Some(path) = arg.strip_prefix("--directory=") {
            base = crate::filesystem::normalize(path, &base, host.home);
            claimed.push(index);
        } else if arg == "--directory" || arg == "--cd" {
            index += 1;
            claimed.push(index);
            if let Some(path) = args.get(index) {
                base = crate::filesystem::normalize(path, &base, "");
            }
        } else if !cluster && !arg.starts_with('-') {
            operands.push(arg.clone());
            claimed.push(index);
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
    claimed
}

fn operand_value(word: &Word) -> Option<&str> {
    let mut value = word.value.as_str();
    if value.starts_with('-') {
        value = value.split_once('=')?.1;
    }
    if let Some(path) = value.strip_prefix('@') {
        return (!path.is_empty()).then_some(path);
    }
    if let Some(at) = value.find('@') {
        let prefix = &value[..at];
        if (prefix.ends_with('=') || prefix.ends_with(':'))
            && prefix
                .trim_end_matches(['=', ':'])
                .chars()
                .all(|c| !matches!(c, '=' | '@') && !space(c))
            && !prefix.trim_end_matches(['=', ':']).is_empty()
        {
            value = &value[at + 1..];
        }
    }
    (!value.is_empty()).then_some(value)
}

fn space(c: char) -> bool {
    c.is_whitespace() && c != '\u{85}' || c == '\u{feff}'
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
    let mut fixed = false;
    let mut patterns = Vec::new();
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
                "include" if program == "rg" => effects.include_advice = true,
                "fixed-strings" => fixed = true,
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
                    fixed |= ch == 'F';
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
                            Some(
                                arg.with_text(
                                    if program == "rg" {
                                        tail.trim_start_matches('=')
                                    } else {
                                        tail
                                    }
                                    .to_owned(),
                                ),
                            )
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
                        patterns.push(value.text);
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
        patterns.push(operands.remove(0).text);
    }
    // native/rules/workflow.go:24-32 applies BRE advice only to pattern roles.
    effects.bre_advice = program == "rg"
        && !fixed
        && patterns.iter().any(|pattern| {
            pattern
                .as_bytes()
                .windows(2)
                .enumerate()
                .any(|(index, bytes)| {
                    bytes == b"\\|" && (index == 0 || pattern.as_bytes()[index - 1] != b'\\')
                })
        });
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
                            &format!(
                                "{}/{}",
                                if root.globs || root.shell_matches {
                                    root.text.clone()
                                } else {
                                    crate::filesystem::literal_glob_root(&root.text)
                                },
                                glob.rsplit('/').next().unwrap_or(glob)
                            ),
                            cwd,
                            host.home,
                        ),
                        Effect::Read,
                        Walk::None,
                        Via::Operand,
                    );
                    target.glob = true;
                    target.glob_hidden = program == "grep" || hidden;
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
            // Go's uninspectable-code owner recognizes a braced HOME prefix
            // as one token (native/rules/appdata.go:59).
            if code[cursor..]
                .get(..7)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("${HOME}"))
            {
                cursor += 7;
            } else {
                cursor += ch.len_utf8();
            }
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
                let home = ["$HOME/", "${HOME}/"].into_iter().find(|prefix| {
                    token
                        .get(..prefix.len())
                        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
                });
                paths.push(home.map_or_else(
                    || token.to_owned(),
                    |prefix| format!("~/{}", &token[prefix.len()..]),
                ));
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
            if source == "D=public true" {
                assert!(effects.targets.is_empty(), "{source}: {effects:?}");
            } else {
                assert_eq!(effects.targets.len(), 1, "{source}: {effects:?}");
                let target = &effects.targets[0];
                assert_eq!(target.path, command.cwd);
                assert_eq!(target.effect, Effect::Enter);
                assert_eq!(target.via, Via::Cwd);
                assert_eq!(target.walk, Walk::None);
            }
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
        let rows: Vec<_> = packet["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["partition"] == "use")
            .collect();
        assert!(!rows.is_empty(), "missing compound use partition");
        for row in rows {
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
            let targets = targets(source);
            assert_eq!(targets.len(), 1);
            assert_eq!(
                targets[0].path,
                if source == "ls .env" {
                    "/p/.env"
                } else {
                    "/p/public"
                }
            );
            assert!(targets.iter().all(|target| target.effect == Effect::List));
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
        let rows: Vec<_> = packet["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["role"].is_string())
            .collect();
        assert!(!rows.is_empty(), "missing P7 role dependency partition");
        for row in rows {
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
