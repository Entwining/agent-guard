use super::*;

pub(super) fn child(command: &CommandRecord, argv: &[Word], cwd: &str) -> CommandRecord {
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

pub(super) fn infer_wrapper(
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
    let mut environment = command.environment.clone();
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
        if program == "env" {
            if arg == "-i" || arg == "--ignore-environment" {
                crate::shell::EnvironmentChange::Clear.apply(&mut environment);
            } else if arg == "-u"
                && let Some(name) = args.get(index + 1)
            {
                crate::shell::EnvironmentChange::Unset(name.text.clone()).apply(&mut environment);
            } else if let Some(change) = crate::shell::EnvironmentChange::assignment(arg) {
                change.apply(&mut environment);
            }
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
    let mut nested = child(command, &args[index..], &cwd);
    nested.environment = environment;
    let mut result = infer_at(&nested, &cwd, host, depth + 1);
    #[cfg(test)]
    {
        effects.owner_visits += result.owner_visits;
        effects.argv_words += result.argv_words;
    }
    if cwd != command.cwd {
        for target in &mut result.targets {
            target.path = at(&target.path, &cwd);
        }
    }
    effects.consumes_listing = program == "xargs"
        && args
            .get(index)
            .is_some_and(|name| xargs_content_consumer(name));
    merge_effects(effects, result);
}

pub(super) fn at(path: &str, base: &str) -> String {
    if path.starts_with('/') || path.starts_with('~') {
        path.to_owned()
    } else {
        format!("{base}/{path}")
    }
}

pub(super) fn xargs_content_consumer(name: &str) -> bool {
    [
        "cat", "head", "tail", "less", "more", "bat", "sed", "awk", "jq", "yq", "base64", "xxd",
        "od", "strings", "sort", "uniq", "cut", "nl", "sh", "bash", "zsh",
    ]
    .contains(&name)
}

fn merge_effects(effects: &mut Effects, result: Effects) {
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
