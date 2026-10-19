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
        let (code, mut claimed) = interpreter_code(program, args);
        for code in code {
            if code.contains("json.load") {
                effects.gaps.push(CoverageGap::UnresolvedTarget);
            }
            effects.inline.push(code);
        }
        // Runtime options end at `--` or at the entry script, which `-` names
        // for stdin. An option value taken for the entry script only ends them
        // early, which reads the separate file word below instead of using it.
        let mut options = true;
        for (index, arg) in args.iter().enumerate() {
            if claimed.contains(&index) {
                continue;
            }
            let env_file = if let Some(path) = arg
                .strip_prefix("--env-file=")
                .or_else(|| arg.strip_prefix("--env-file-if-exists="))
            {
                Some(arg.with_text(path.to_owned()))
            } else if options
                && ["node", "tsx"].contains(&program)
                && ["--env-file", "--env-file-if-exists"].contains(&arg.as_str())
                && let Some(path) = args.get(index + 1)
            {
                // Node also takes the file as the next word.
                claimed.push(index + 1);
                Some(path.clone())
            } else {
                None
            };
            if let Some(path) = env_file {
                effects
                    .targets
                    .push(Target::from_word(&path, cwd, host, Effect::Use, Walk::None));
            } else if arg == "--" || arg == "-" {
                options = false;
            } else if !arg.starts_with('-') {
                options = false;
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
