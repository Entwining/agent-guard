use super::*;
mod builtins;
mod executables;
mod readers;

pub(super) fn infer(
    program: &str,
    args: &[Word],
    command: &CommandRecord,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
    depth: usize,
) -> (Option<Walk>, Vec<usize>) {
    let index = command.program.unwrap_or(0);
    let mut generic_walk = None;
    let mut claimed = Vec::new();
    match program {
        "__observed_stream__" => effects.gaps.push(CoverageGap::UnresolvedTarget),
        "printf" | "echo" | "print" | "true" | "false" | ":" | "unset" | "local" | "break"
        | "continue" | "return" | "shift" | "tr" | "kill" | "mktemp" | "df" | "stat" | "test"
        | "[" | "chmod" | "chown" | "chgrp" | "chflags" | "touch" | "rmdir" | "mkdir" | "wc"
        | "file" | "shasum" | "sha1sum" | "sha256sum" | "md5" | "md5sum" | "cksum" | "realpath"
        | "readlink" | "basename" | "dirname" | "cd" | "pushd" | "popd" | "setopt" | "unsetopt"
        | "emulate" | "set" | "printenv" => {
            builtins::infer(program, args, command, cwd, host, effects)
        }
        "read"
            if command.pipeline.is_some()
                || command.stdin == crate::record::Stdin::Inherited
                || command.redirects.iter().any(|redirect| {
                    matches!(
                        redirect.direction,
                        Direction::Heredoc | Direction::Herestring
                    ) && !redirect.expands
                }) => {}
        "hash"
            if command.shell
                && args.iter().any(|arg| {
                    arg.strip_prefix('-')
                        .is_some_and(|flags| flags.contains('d'))
                }) =>
        {
            builtins::infer(program, args, command, cwd, host, effects)
        }
        "typeset" | "declare" | "export" if command.shell && !command.argv[index].contains('/') => {
            builtins::infer(program, args, command, cwd, host, effects)
        }
        "cat" | "head" | "tail" | "less" | "more" | "bat" | "sort" | "uniq" | "cut" | "nl"
        | "base64" | "xxd" | "od" | "strings" => {
            readers::files(args, command, effects);
            generic_walk = Some(Walk::None);
        }
        "ls" => readers::listing(args, cwd, host, effects),
        "rg" | "grep" | "ag" | "ack" => {
            infer_search(program, args, cwd, host, effects);
            if command.unresolved() {
                effects.gaps.push(CoverageGap::UnresolvedTarget);
            }
        }
        "xargs" | "env" => infer_wrapper(program, args, command, cwd, host, effects, depth),
        "fd" | "tree" | "du" | "find" => {
            infer_listing(program, args, command, cwd, host, effects, depth)
        }
        "tar" => {
            claimed = infer_tar(args, cwd, host, effects);
            generic_walk = Some(Walk::Visible);
        }
        "jq" | "yq" => {
            claimed = readers::filter(program, args);
            generic_walk = Some(Walk::Visible);
        }
        "rm" | "mv" | "ln" => effects
            .targets
            .extend(resource_changes::infer(program, args, cwd, host)),
        "git" => readers::git(args, cwd, host, effects, command),
        "cp" | "install" | "rsync" | "tee" | "ssh-add" | "dotenvx" | "ssh" | "scp" | "sftp"
        | "ssh-keygen" | "dd" | "kubectl" | "npm" | "curl" | "wget" | "docker" => effects
            .targets
            .extend(clients::infer(program, args, cwd, host)),
        "ctags" => {
            effects.gaps.push(CoverageGap::UnknownProgram {
                program: program.to_owned(),
            });
            effects
                .targets
                .extend(clients::infer(program, args, cwd, host));
        }
        "python" | "python3" | "node" | "bun" | "ruby" | "perl" | "php" | "osascript" | "lua"
        | "deno" => executables::interpreter(program, args, cwd, host, effects, command, depth),
        "bash" | "zsh" | "sh" | "dash" | "ksh" | "csh" | "tcsh" => {
            executables::shell(args, command, cwd, host, effects)
        }
        // The shell evaluator already observes builtin eval in its binding/cwd
        // scope. Replaying it here both loses that scope and doubles nested work.
        "eval" if !command.shell => effects
            .code
            .push(args.iter().map(Word::as_str).collect::<Vec<_>>().join(" ")),
        "eval" => {}
        _ => {
            readers::unknown(program, args, effects);
            generic_walk = Some(Walk::Visible);
        }
    }
    (generic_walk, claimed)
}

fn read(word: &Word, cwd: &str, host: HostFacts<'_>, recursive: bool) -> Target {
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
}
