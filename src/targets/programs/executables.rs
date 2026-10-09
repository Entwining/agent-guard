use super::super::*;
use super::read;

pub(super) fn interpreter(
    program: &str,
    args: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
    command: &CommandRecord,
    depth: usize,
) {
    if program == "perl"
        && let Some(forwarded) = perl_exec_argv(args)
    {
        // The recognized -e/-E source comes from argv, regardless of stdin.
        // Its exec list inherits stdin; the executed program owns its roles.
        effects.independent_arguments = true;
        if forwarded[0].contains('/') {
            effects.targets.push(read(&forwarded[0], cwd, host, false));
        }
        infer_forwarded(forwarded, command, cwd, host, effects, depth);
    } else {
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
                effects.targets.push(read(arg, cwd, host, false));
            }
        }
    }
}

pub(super) fn shell(
    args: &[Word],
    command: &CommandRecord,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    // Without flags each argument has its own read role; no combination
    // can select code, consume another argument, or change its target.
    effects.independent_arguments = args.iter().all(|arg| !arg.starts_with('-'));
    let mut claimed = Vec::new();
    if let Some(index) = args.iter().position(|s| {
        s.starts_with('-') && s[1..].chars().all(|c| c.is_ascii_lowercase()) && s.contains('c')
    }) {
        if let Some(code) = args.get(index + 1) {
            if code.role != crate::record::Role::ObservedShellCode {
                effects.code.push(code.text.clone());
            }
            claimed.push(index + 1);
        }
    } else if command.stdin != crate::record::Stdin::Shell {
        effects.gaps.push(CoverageGap::UnresolvedTarget);
    }
    for (index, arg) in args.iter().enumerate() {
        if !claimed.contains(&index) && !arg.starts_with('-') {
            effects.targets.push(read(arg, cwd, host, false));
        }
    }
}
