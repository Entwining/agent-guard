use super::{operand_value, space};
use crate::record::{Effect, HostFacts, Target, Via, Walk, Word};

struct Spec {
    operand: Effect,
    walk: Walk,
    options: &'static [(&'static str, Effect)],
    remote: bool,
}

fn spec(program: &str) -> Spec {
    let mut spec = Spec {
        operand: Effect::Read,
        walk: Walk::Visible,
        options: &[],
        remote: false,
    };
    match program {
        "ssh" => spec.operand = Effect::Name,
        "scp" => spec.remote = true,
        "dd" => spec.walk = Walk::None,
        "ssh-keygen" => spec.options = &[("-f", Effect::Use)],
        "kubectl" => spec.options = &[("--kubeconfig", Effect::Use)],
        "npm" => spec.options = &[("--userconfig", Effect::Use)],
        "curl" => {
            spec.operand = Effect::Name;
            spec.options = CURL_USE;
        }
        "wget" => {
            spec.operand = Effect::Name;
            spec.options = WGET_USE;
        }
        "docker" => {
            spec.operand = Effect::Name;
            spec.options = &[("--env-file", Effect::Use)];
        }
        _ => {}
    }
    spec
}

struct Context<'a> {
    words: &'a [Word],
    cwd: &'a str,
    host: HostFacts<'a>,
    spec: Spec,
    claimed: Vec<bool>,
    targets: Vec<Target>,
}

impl<'a> Context<'a> {
    fn text(&self, index: usize) -> &'a str {
        self.words.get(index).map_or("", Word::as_str)
    }
    fn add(
        &mut self,
        path: &str,
        index: Option<usize>,
        effect: Effect,
        quoted: Option<bool>,
    ) -> &mut Target {
        let mut word = self
            .words
            .get(index.unwrap_or(usize::MAX))
            .cloned()
            .unwrap_or_else(|| Word::literal(String::new()));
        word.text = path.into();
        if let Some(quoted) = quoted {
            word.raw = if quoted { "\"" } else { "" }.into();
        }
        let mut target = Target::from_word(&word, self.cwd, self.host, effect, self.spec.walk);
        target.via = Via::Option;
        if let Some(index) = index {
            self.claimed[index] = true;
        }
        let at = self.targets.len();
        self.targets.push(target);
        &mut self.targets[at]
    }
    fn option_effect(&self, index: usize) -> Option<Effect> {
        let value = &self.words[index].value;
        let previous = self.text(index.wrapping_sub(1));
        let key = if value.starts_with('-') {
            value.split_once('=').map_or("", |(key, _)| key)
        } else if previous.starts_with('-') && !previous.starts_with("--") {
            previous
                .get(previous.len().saturating_sub(1)..)
                .unwrap_or("")
        } else {
            previous
        };
        self.spec.options.iter().find_map(|(option, effect)| {
            (*option == key || key.len() == 1 && option.strip_prefix('-') == Some(key))
                .then_some(*effect)
        })
    }
    fn fallback(&mut self) {
        let last = self
            .words
            .iter()
            .enumerate()
            .filter(|(i, word)| !word.starts_with('-') && self.option_effect(*i).is_none())
            .map(|(i, _)| i)
            .next_back();
        let sends = self.spec.remote && self.words.iter().any(|word| remote(&word.value));
        for (index, word) in self.words.iter().enumerate() {
            if self.claimed[index] {
                continue;
            }
            if let Some(flags) = word.strip_prefix('-').filter(|s| !s.starts_with('-'))
                && let Some((at, (_, effect))) = flags.char_indices().find_map(|(at, letter)| {
                    self.spec
                        .options
                        .iter()
                        .find(|(option, _)| option.strip_prefix('-') == Some(&letter.to_string()))
                        .map(|option| (at, option))
                })
            {
                let value = &flags[at + 1..];
                if !value.is_empty() {
                    let mut target = Target::from_word(
                        &word.with_text(value.into()),
                        self.cwd,
                        self.host,
                        *effect,
                        self.spec.walk,
                    );
                    target.via = Via::Option;
                    target.sends = sends;
                    self.targets.push(target);
                    continue;
                }
            }
            let Some(value) = operand_value(word) else {
                continue;
            };
            let mut effect = self.option_effect(index).unwrap_or(self.spec.operand);
            if self.spec.remote && Some(index) == last && !word.globs {
                effect = Effect::Write;
            }
            if self.spec.remote && remote(&word.value) {
                effect = Effect::Name;
            }
            let mut target = Target::from_word(
                &word.with_text(value.into()),
                self.cwd,
                self.host,
                effect,
                self.spec.walk,
            );
            target.sends = sends;
            self.targets.push(target);
        }
    }
}

fn remote(value: &str) -> bool {
    value.starts_with("rsync://")
        || value.split_once(':').is_some_and(|(host, _)| {
            let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
            !host.is_empty() && !host.contains(['/', '@', ':'])
        })
}

pub(super) fn infer(program: &str, words: &[Word], cwd: &str, host: HostFacts<'_>) -> Vec<Target> {
    let mut context = Context {
        words,
        cwd,
        host,
        spec: spec(program),
        claimed: vec![false; words.len()],
        targets: Vec::new(),
    };
    match program {
        "ssh" | "scp" | "sftp" => ssh(program, &mut context),
        "curl" => curl(&mut context),
        "wget" => wget(&mut context),
        "docker" => docker(&mut context),
        "dd" => {
            for (index, word) in words.iter().enumerate() {
                context.claimed[index] = true;
                if let Some(path) = word.strip_prefix("if=") {
                    context.add(path, Some(index), Effect::Read, None);
                } else if let Some(path) = word.strip_prefix("of=") {
                    context.add(path, Some(index), Effect::Write, None);
                }
            }
        }
        _ => {}
    }
    context.fallback();
    context.targets
}

pub(super) const CURL_VALUE_LETTERS: &str = "AbcCdDeEFHKmoPQrTtuUwxXyYz";
const CURL_USE: &[(&str, Effect)] = &[
    ("--cacert", Effect::Use),
    ("--capath", Effect::Use),
    ("--cert", Effect::Use),
    ("--key", Effect::Use),
    ("-E", Effect::Use),
    ("--netrc-file", Effect::Use),
    ("--crlfile", Effect::Use),
    ("--egd-file", Effect::Use),
    ("--knownhosts", Effect::Use),
    ("--proxy-cacert", Effect::Use),
    ("--proxy-capath", Effect::Use),
    ("--proxy-cert", Effect::Use),
    ("--proxy-crlfile", Effect::Use),
    ("--proxy-key", Effect::Use),
    ("--random-file", Effect::Use),
    ("--pubkey", Effect::Use),
    ("--pinnedpubkey", Effect::Use),
    ("--proxy-pinnedpubkey", Effect::Use),
    ("--unix-socket", Effect::Use),
];
const WGET_USE: &[(&str, Effect)] = &[
    ("--ca-certificate", Effect::Use),
    ("--ca-directory", Effect::Use),
    ("--certificate", Effect::Use),
    ("--private-key", Effect::Use),
    ("--crl-file", Effect::Use),
    ("--random-file", Effect::Use),
];
const CURL_LONG: &[&str] = &[
    "data",
    "data-ascii",
    "data-binary",
    "data-urlencode",
    "json",
    "form",
    "header",
    "proxy-header",
    "url-query",
    "variable",
    "upload-file",
    "config",
    "output",
    "dump-header",
    "write-out",
    "cookie",
    "etag-compare",
    "cookie-jar",
    "etag-save",
    "libcurl",
    "stderr",
    "hsts",
    "alt-svc",
    "trace",
    "trace-ascii",
    "ssl-sessions",
];
const CURL_WRITES: &[&str] = &[
    "o",
    "output",
    "D",
    "dump-header",
    "c",
    "cookie-jar",
    "etag-save",
    "libcurl",
    "stderr",
    "hsts",
    "alt-svc",
    "trace",
    "trace-ascii",
    "ssl-sessions",
];

fn curl(context: &mut Context<'_>) {
    let mut index = 0;
    let mut remote_name = false;
    let mut output_dir = None;
    while let Some(word) = context.words.get(index) {
        if word == "--" {
            break;
        }
        let text = word.as_str();
        remote_name |= ["--remote-name", "--remote-name-all"].contains(&text)
            || text.starts_with('-') && !text[1..].contains('-') && text.ends_with('O');
        if text == "--output-dir" || text.starts_with("--output-dir=") {
            if !text.contains('=') {
                index += 1;
            }
            output_dir = (index < context.words.len()).then_some(index);
            if let Some(index) = output_dir {
                context.claimed[index] = true;
            }
            index += 1;
            continue;
        }
        let url = if text
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("--url="))
        {
            &text[6..]
        } else {
            text
        };
        if url
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
        {
            let path = url[5..].strip_prefix("//").unwrap_or(&url[5..]);
            let mut decoded = String::new();
            let mut at = 0;
            while at < path.len() {
                if path.as_bytes()[at] == b'%'
                    && let Some(pair) = path.get(at + 1..at + 3)
                    && let Ok(value) = u8::from_str_radix(pair, 16)
                {
                    decoded.push(char::from(value));
                    at += 3;
                } else {
                    let ch = path[at..].chars().next().unwrap_or_default();
                    decoded.push(ch);
                    at += ch.len_utf8();
                }
            }
            let target = context.add(&decoded, Some(index), Effect::Read, None);
            target.via = Via::Operand;
            target.glob = decoded.contains(['[', '{']);
            index += 1;
            continue;
        }
        let mut key = "";
        let mut value = "";
        if let Some(option) = text.strip_prefix("--")
            && CURL_LONG.contains(&option.split('=').next().unwrap_or(""))
        {
            key = option.split('=').next().unwrap_or("");
            if let Some((_, attached)) = option.split_once('=') {
                value = attached;
            } else {
                index += 1;
                value = context.text(index);
            }
        } else if let Some(flags) = text
            .strip_prefix('-')
            .filter(|flags| !flags.starts_with('-'))
            && let Some((at, letter)) = flags
                .char_indices()
                .find(|(_, letter)| CURL_VALUE_LETTERS.contains(*letter))
        {
            key = &flags[at..at + letter.len_utf8()];
            value = &flags[at + letter.len_utf8()..];
            if value.is_empty() {
                index += 1;
                value = context.text(index);
            }
        }
        if index >= context.words.len() {
            break;
        }
        match key {
            "d" | "data" | "data-ascii" | "data-binary" | "data-urlencode" | "json" | "H"
            | "header" | "proxy-header" | "url-query" | "variable" => {
                if let Some((_, path)) = value.split_once('@').filter(|(_, path)| !path.is_empty())
                {
                    context
                        .add(path, Some(index), Effect::Read, Some(true))
                        .sends = true;
                }
            }
            "F" | "form" => {
                let file = value.split_once('=').map_or(value, |(_, value)| value);
                let file = file.strip_prefix(['@', '<']).unwrap_or(file);
                let file = file
                    .strip_prefix('"')
                    .and_then(|file| file.split_once('"').map(|(file, _)| file))
                    .unwrap_or_else(|| file.split(';').next().unwrap_or(""));
                context
                    .add(file, Some(index), Effect::Read, Some(true))
                    .sends = true;
            }
            "T" | "upload-file" | "K" | "config" | "etag-compare" => {
                context.add(value, Some(index), Effect::Read, None).sends = true;
            }
            "w" | "write-out" if value.starts_with('@') && value.len() > 1 && value != "@-" => {
                context.add(&value[1..], Some(index), Effect::Read, Some(true));
            }
            "b" | "cookie" if !value.is_empty() && !value.contains('=') => {
                context.add(value, Some(index), Effect::Use, None);
            }
            key if CURL_WRITES.contains(&key) && !value.is_empty() => {
                context.claimed[index] = true;
                if value != "-" {
                    context.add(value, Some(index), Effect::Write, None);
                }
            }
            _ => {}
        }
        index += 1;
    }
    if remote_name {
        let path = output_dir.map_or(".", |index| {
            context
                .text(index)
                .strip_prefix("--output-dir=")
                .unwrap_or(context.text(index))
        });
        context.add(path, output_dir, Effect::Write, None).walk = Walk::None;
    }
}

const WGET_WRITES: &[&str] = &[
    "output-document",
    "output-file",
    "append-output",
    "directory-prefix",
    "save-cookies",
    "warc-file",
    "hsts-file",
];
const WGET_LONG: &[&str] = &[
    "post-file",
    "body-file",
    "input-file",
    "output-document",
    "output-file",
    "append-output",
    "directory-prefix",
    "save-cookies",
    "warc-file",
    "hsts-file",
    "execute",
    "config",
    "load-cookies",
];

fn wget(context: &mut Context<'_>) {
    let mut placed = false;
    for (index, word) in context.words.iter().enumerate() {
        let text = word.as_str();
        placed |= text == "--spider";
        let (mut key, attached) = if let Some(option) = text.strip_prefix("--")
            && WGET_LONG.contains(&option.split('=').next().unwrap_or(""))
        {
            (
                option.split('=').next().unwrap_or(""),
                option.split_once('=').map(|(_, value)| value),
            )
        } else if let Some(flags) = text.strip_prefix('-') {
            let hit = flags
                .char_indices()
                .take_while(|(_, letter)| letter.is_ascii_alphabetic())
                .find(|(_, letter)| "ieOPoa".contains(*letter));
            if let Some((at, letter)) = hit {
                let key = match letter {
                    'i' => "input-file",
                    'e' => "execute",
                    'O' => "output-document",
                    'P' => "directory-prefix",
                    'o' => "output-file",
                    _ => "append-output",
                };
                let value = &flags[at + 1..];
                (key, (!value.is_empty()).then_some(value))
            } else {
                continue;
            }
        } else {
            continue;
        };
        let holder = if attached.is_some() { index } else { index + 1 };
        if holder >= context.words.len() {
            continue;
        }
        let mut path = attached.unwrap_or_else(|| context.text(holder));
        if key == "execute" {
            let Some((name, value)) = path.trim_start_matches(space).split_once('=') else {
                continue;
            };
            let name = name.trim_end_matches(space);
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphabetic() || matches!(c, '_' | '-'))
            {
                continue;
            }
            key = match name.to_ascii_lowercase().replace(['_', '-'], "").as_str() {
                "postfile" => "post-file",
                "bodyfile" => "body-file",
                "input" => "input-file",
                "outputdocument" => "output-document",
                "logfile" => "output-file",
                "dirprefix" => "directory-prefix",
                "loadcookies" => "load-cookies",
                "savecookies" => "save-cookies",
                "warcfile" => "warc-file",
                "hstsfile" => "hsts-file",
                _ => continue,
            };
            path = value.trim_start_matches(space);
        }
        context.claimed[holder] = true;
        placed |= matches!(key, "output-document" | "directory-prefix");
        if key == "output-document" && path == "-" {
            continue;
        }
        let effect = if WGET_WRITES.contains(&key) {
            Effect::Write
        } else {
            Effect::Read
        };
        let target = context.add(path, Some(holder), effect, None);
        target.walk = Walk::None;
        target.sends = matches!(key, "post-file" | "body-file" | "config");
    }
    if !placed {
        context.add(".", None, Effect::Write, None).walk = Walk::None;
    }
}

fn ssh(program: &str, context: &mut Context<'_>) {
    let letters = match program {
        "ssh" => "BDEFIJLOPQRSWbceilmopw",
        "scp" => "DFJPSXcilo",
        _ => "BDFJPRSXbcilos",
    };
    let mut index = 0;
    let mut operands = 0;
    while let Some(word) = context.words.get(index) {
        if word == "--" {
            break;
        }
        if !word.starts_with('-') || word.len() < 2 {
            operands += 1;
            if operands == if program == "ssh" { 2 } else { 1 } {
                break;
            }
            index += 1;
            continue;
        }
        let Some((at, letter)) = word
            .char_indices()
            .skip(1)
            .find(|(_, c)| letters.contains(*c))
        else {
            index += 1;
            continue;
        };
        let glued = &word[at + letter.len_utf8()..];
        let value = if glued.is_empty() {
            index += 1;
            context.text(index)
        } else {
            glued
        };
        if index >= context.words.len() {
            break;
        }
        let mut effect = match letter {
            'i' | 'F' | 'S' => Some(Effect::Use),
            'E' => Some(Effect::Write),
            'b' if program == "sftp" && value != "-" => Some(Effect::Read),
            _ => None,
        };
        let mut paths = vec![value.to_owned()];
        if letter == 'o' {
            paths.clear();
            effect = None;
            let name_end = value
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(value.len());
            let rest = &value[name_end..];
            let rest = if let Some(rest) = rest.trim_start_matches(space).strip_prefix('=') {
                Some(rest.trim_start_matches(space))
            } else if rest.chars().next().is_some_and(space) {
                Some(rest.trim_start_matches(space))
            } else {
                None
            };
            if let Some(rest) = rest.filter(|rest| !rest.is_empty()) {
                effect = match value[..name_end].to_ascii_lowercase().as_str() {
                    "identityfile"
                    | "certificatefile"
                    | "globalknownhostsfile"
                    | "revokedhostkeys"
                    | "pkcs11provider" => Some(Effect::Use),
                    "userknownhostsfile" => Some(Effect::Write),
                    _ => None,
                };
                let mut remaining = rest;
                while !remaining.is_empty() {
                    remaining = remaining.trim_start_matches(space);
                    if remaining.is_empty() {
                        break;
                    }
                    let end = if let Some(quoted) = remaining.strip_prefix('"') {
                        quoted.find('"').map_or_else(
                            || remaining.find(space).unwrap_or(remaining.len()),
                            |at| at + 2,
                        )
                    } else {
                        remaining.find(space).unwrap_or(remaining.len())
                    };
                    let mut path = remaining[..end].replace('"', "");
                    for prefix in ["%d", "${HOME}"] {
                        if path == prefix
                            || path
                                .strip_prefix(prefix)
                                .is_some_and(|tail| tail.starts_with('/'))
                        {
                            path = format!("~{}", &path[prefix.len()..]);
                        }
                    }
                    paths.push(path);
                    remaining = &remaining[end..];
                }
            }
        }
        if let Some(effect) = effect {
            for path in paths {
                context.add(&path, Some(index), effect, (letter == 'o').then_some(false));
            }
        }
        index += 1;
    }
}

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

fn docker(context: &mut Context<'_>) {
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
        let volume = if let Some(option) = text.strip_prefix("--") {
            let (key, value) = option.split_once('=').unwrap_or((option, ""));
            ["volume", "mount"].contains(&key).then_some((key, value))
        } else {
            short_value(text, 'v').map(|value| ("volume", value.strip_prefix('=').unwrap_or(value)))
        };
        let Some((key, value)) = volume else {
            continue;
        };
        let holder = if value.is_empty() { index + 1 } else { index };
        if holder >= context.words.len() {
            continue;
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
}

fn short_value(text: &str, wanted: char) -> Option<&str> {
    text.strip_prefix('-')?
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_alphabetic())
        .find(|(_, c)| *c == wanted)
        .map(|(at, c)| &text[at + 1 + c.len_utf8()..])
}
