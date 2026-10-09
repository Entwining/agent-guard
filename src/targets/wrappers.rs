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

pub(super) fn infer_forwarded(
    argv: &[Word],
    command: &CommandRecord,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
    depth: usize,
) {
    let result = infer_at(&child(command, argv, cwd), cwd, host, depth + 1);
    #[cfg(test)]
    {
        effects.owner_visits += result.owner_visits;
        effects.argv_words += result.argv_words;
    }
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
