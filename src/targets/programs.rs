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
) -> Option<Operands> {
    let index = command.program.unwrap_or(0);
    let unknown_conventions = |walk: Walk, claimed: Vec<usize>| {
        Some(Operands {
            walk,
            claimed,
            literal: false,
        })
    };
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
                || matches!(
                    command.stdin,
                    crate::record::Stdin::Inherited | crate::record::Stdin::Stream
                )
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
            return Some(readers::files(program, args, command, cwd, host, effects));
        }
        "ls" => readers::listing(args, cwd, host, effects),
        "rg" | "grep" | "ag" | "ack" => {
            infer_search(program, args, command, cwd, host, effects);
            if command.unresolved() {
                effects.gaps.push(CoverageGap::UnresolvedTarget);
            }
        }
        "fd" | "tree" | "du" | "find" => {
            infer_listing(program, args, command, cwd, host, effects, depth)
        }
        "tar" => return unknown_conventions(Walk::Visible, infer_tar(args, cwd, host, effects)),
        "jq" | "yq" => return unknown_conventions(Walk::Visible, readers::filter(program, args)),
        "gh" | "aws" | "rex" | "sessidx" => {
            readers::unknown(program, args, effects);
            return unknown_conventions(
                Walk::Visible,
                match program {
                    "rex" => readers::rex_names(args),
                    "sessidx" => readers::sessidx_queries(args),
                    _ => readers::output_query(program, args),
                },
            );
        }
        "rm" | "mv" | "ln" => resource_changes::infer(program, args, cwd, host, effects),
        "git" => infer_git(args, cwd, host, effects),
        "ctags" => {
            effects.gaps.push(CoverageGap::UnknownProgram {
                program: program.to_owned(),
            });
            clients::infer(program, args, cwd, host, effects);
        }
        "python" | "python3" | "node" | "tsx" | "bun" | "ruby" | "perl" | "php" | "osascript"
        | "lua" | "deno" => {
            executables::interpreter(program, args, cwd, host, effects, command, depth)
        }
        "bash" | "zsh" | "sh" | "dash" | "ksh" | "csh" | "tcsh" => {
            executables::shell(program, args, command, cwd, host, effects)
        }
        "eval" => executables::eval(args, command, effects),
        _ => {
            if !clients::infer(program, args, cwd, host, effects) {
                readers::unknown(program, args, effects);
                return Some(readers::options(
                    program,
                    args,
                    Walk::Visible,
                    cwd,
                    host,
                    effects,
                ));
            }
        }
    }
    None
}

fn read(word: &Word, cwd: &str, host: HostFacts<'_>, recursive: bool) -> Target {
    Target::from_word(
        word,
        cwd,
        host,
        Effect::Read,
        if recursive { Walk::Visible } else { Walk::None },
    )
}
