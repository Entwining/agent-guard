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
            if options && effects.unproven.is_none() && arg.option_unknown() {
                effects.unproven = Some(index);
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

// The shell evaluator already observes builtin eval of exact code in its
// binding/cwd scope. Replaying it here both loses that scope and doubles
// nested work; a representative is only lexical evidence.
pub(super) fn eval(args: &[Word], command: &CommandRecord, effects: &mut Effects) {
    let code = args.iter().map(Word::as_str).collect::<Vec<_>>().join(" ");
    if args.iter().any(|arg| arg.role == Role::ObservedShellCode) {
        // Its source owner re-expanded it in this shell's scope.
    } else if args.iter().any(|arg| arg.exact().is_none()) {
        effects.inline.push(code);
    } else if !command.shell {
        effects.code.push(code);
    }
}

pub(super) fn shell(
    program: &str,
    args: &[Word],
    command: &CommandRecord,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    let Some(invocation) = shell_invocation(program, args) else {
        return;
    };
    // Without options each argument has its own read role; no combination
    // can select code, consume another argument, or change its target.
    effects.independent_arguments =
        matches!(invocation.source, ShellSource::Script(0)) || args.is_empty();
    for (index, effect) in &invocation.values {
        if let Some(effect) = effect {
            effects.targets.push(Target::from_word(
                &args[*index],
                cwd,
                host,
                *effect,
                Walk::None,
            ));
        }
    }
    let mut operands = invocation.operands..args.len();
    let mut code = None;
    match &invocation.source {
        ShellSource::Code(Some(index)) => {
            let word = &args[*index];
            if word.role == Role::ObservedShellCode {
                // Its source owner inspected it in the child scope.
            } else if word.exact().is_none() {
                // A representative is lexical evidence of the code, not
                // code to parse.
                effects.inline.push(word.text.clone());
                effects.gaps.push(CoverageGap::UnresolvedTarget);
            } else {
                effects.code.push(word.text.clone());
            }
            code = Some(*index);
        }
        ShellSource::Uncertain(candidates) => {
            // Each later word keeps a read role from the unproven word on;
            // that word may itself be the script.
            effects.unproven = Some(invocation.operands);
            operands.end = (invocation.operands + 1).min(args.len());
            for word in candidates.iter().map(|index| &args[*index]) {
                if word.exact().is_none() {
                    effects.inline.push(word.text.clone());
                    effects.gaps.push(CoverageGap::UnresolvedTarget);
                } else if !invocation.posix {
                    effects.code.push(word.text.clone());
                }
            }
        }
        ShellSource::Code(None) | ShellSource::Stdin | ShellSource::Script(_) => {}
    }
    if !matches!(invocation.source, ShellSource::Code(_))
        && command.stdin != crate::record::Stdin::Shell
    {
        effects.gaps.push(CoverageGap::UnresolvedTarget);
    }
    for index in operands.filter(|index| Some(*index) != code) {
        effects.targets.push(read(&args[index], cwd, host, false));
    }
}
