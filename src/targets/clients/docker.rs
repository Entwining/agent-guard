use super::*;

const DOCKER_GLOBAL: &[&str] = &[
    "-H",
    "--host",
    "-c",
    "--context",
    "-l",
    "--log-level",
    "--config",
    "--tlscacert",
    "--tlscert",
    "--tlskey",
];
const DOCKER_FLAGS: &[&str] = &[
    "--detach",
    "--help",
    "--init",
    "--interactive",
    "--no-healthcheck",
    "--oom-kill-disable",
    "--privileged",
    "--publish-all",
    "--quiet",
    "--read-only",
    "--rm",
    "--sig-proxy",
    "--tty",
    "--use-api-socket",
];
const COMPOSE_VALUES: &[&str] = &[
    "-f",
    "--file",
    "-p",
    "--project-name",
    "--project-directory",
    "--profile",
    "--env-file",
    "--ansi",
    "--parallel",
    "--progress",
];

fn docker_command_start(context: &Context<'_>, start: usize) -> usize {
    let mut index = start + 1;
    while let Some(word) = context.words.get(index) {
        if !word.starts_with('-') {
            return index;
        }
        if word == "--" {
            return index + 1;
        }
        if word.starts_with("--") {
            let next_is_option = context
                .text(index + 1)
                .strip_prefix('-')
                .and_then(|s| s.chars().next())
                .is_some_and(|c| !c.is_ascii_digit());
            if !word.contains('=')
                && (word == "--entrypoint"
                    || !DOCKER_FLAGS.contains(&word.as_str()) && !next_is_option)
            {
                index += 1;
            }
        } else {
            for (at, letter) in word.char_indices().skip(1) {
                if !"diqPt".contains(letter) {
                    if at + letter.len_utf8() == word.len() {
                        index += 1;
                    }
                    break;
                }
            }
        }
        index += 1;
    }
    context.words.len()
}

pub(super) fn docker(context: &mut Context<'_>) {
    let mut start = 0;
    while context.text(start).starts_with('-') {
        start += if DOCKER_GLOBAL.contains(&context.text(start)) {
            2
        } else {
            1
        };
    }
    if ["container", "image", "buildx"].contains(&context.text(start)) {
        start += 1;
    }
    let sub = context.text(start);
    let end = if ["run", "create", "exec"].contains(&sub) {
        docker_command_start(context, start)
    } else {
        context.words.len()
    };
    let mut compose_sub = start + 1;
    while context.text(compose_sub).starts_with('-') {
        compose_sub += if COMPOSE_VALUES.contains(&context.text(compose_sub)) {
            2
        } else {
            1
        };
    }
    for (index, word) in context.words[..end].iter().enumerate() {
        let previous = context.text(index.wrapping_sub(1));
        if previous == "--entrypoint" {
            continue;
        }
        let text = word.as_str();
        operand_targets(context, index, sub, start, text, previous);
        let secret = text
            .strip_prefix("--secret=")
            .or_else(|| (previous == "--secret").then_some(text))
            .unwrap_or("");
        for field in secret.split(',') {
            if let Some(path) = field
                .strip_prefix("src=")
                .or_else(|| field.strip_prefix("source="))
            {
                context.add(path, Some(index), Effect::Read, None);
            }
        }
        host_option(context, index, sub, compose_sub, text);
        volume_option(context, index, text);
    }
}

fn operand_targets(
    context: &mut Context<'_>,
    index: usize,
    sub: &str,
    start: usize,
    text: &str,
    previous: &str,
) {
    if sub == "cp"
        && index > start
        && !text.starts_with('-')
        && !text.split_once(':').is_some_and(|(name, _)| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        })
    {
        let target = context.add(
            text,
            Some(index),
            if index == context.words.len() - 1 {
                Effect::Write
            } else {
                Effect::Read
            },
            None,
        );
        target.via = Via::Operand;
    }
    if sub == "load" {
        let input = text
            .strip_prefix("--input=")
            .or_else(|| text.strip_prefix("-i"))
            .filter(|s| !s.is_empty())
            .or_else(|| ["-i", "--input"].contains(&previous).then_some(text));
        if let Some(input) = input.filter(|s| !s.is_empty()) {
            context.add(input, Some(index), Effect::Read, None);
        }
    }
}

fn host_option(context: &mut Context<'_>, index: usize, sub: &str, compose_sub: usize, text: &str) {
    let host = if let Some(option) = text.strip_prefix("--") {
        let (key, value) = option.split_once('=').unwrap_or((option, ""));
        [
            "file",
            "label-file",
            "cidfile",
            "iidfile",
            "tlscacert",
            "tlscert",
            "tlskey",
            "config",
        ]
        .contains(&key)
        .then_some((key, value))
    } else if sub == "build" || sub == "compose" && index < compose_sub {
        short_value(text, 'f').map(|value| ("f", value.strip_prefix('=').unwrap_or(value)))
    } else {
        None
    };
    if let Some((key, value)) = host {
        let holder = if value.is_empty() { index + 1 } else { index };
        if holder < context.words.len() {
            let path = if value.is_empty() {
                context.text(holder)
            } else {
                value
            };
            context.add(
                path,
                Some(holder),
                if ["cidfile", "iidfile"].contains(&key) {
                    Effect::Write
                } else {
                    Effect::Use
                },
                None,
            );
        }
    }
}

fn volume_option(context: &mut Context<'_>, index: usize, text: &str) {
    let volume = if let Some(option) = text.strip_prefix("--") {
        let (key, value) = option.split_once('=').unwrap_or((option, ""));
        ["volume", "mount"].contains(&key).then_some((key, value))
    } else {
        short_value(text, 'v').map(|value| ("volume", value.strip_prefix('=').unwrap_or(value)))
    };
    let Some((key, value)) = volume else {
        return;
    };
    let holder = if value.is_empty() { index + 1 } else { index };
    if holder >= context.words.len() {
        return;
    }
    let value = if value.is_empty() {
        context.text(holder)
    } else {
        value
    };
    if key == "volume" {
        context.add(
            value.split(':').next().unwrap_or(""),
            Some(holder),
            Effect::Read,
            None,
        );
    } else {
        for field in value.split(',') {
            if let Some(path) = field
                .strip_prefix("src=")
                .or_else(|| field.strip_prefix("source="))
            {
                context.add(path, Some(holder), Effect::Read, None);
            }
        }
    }
}
