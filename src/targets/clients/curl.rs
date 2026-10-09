use super::*;

pub(in crate::targets) const CURL_VALUE_LETTERS: &str = "AbcCdDeEFHKmoPQrTtuUwxXyYz";
pub(super) const CURL_USE: &[(&str, Effect)] = &[
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

pub(super) fn curl(context: &mut Context<'_>) {
    let mut index = 0;
    let mut options = true;
    let mut remote_name = false;
    let mut output_dir = None;
    while let Some(word) = context.words.get(index) {
        if options && word == "--" {
            options = false;
            index += 1;
            continue;
        }
        let text = word.as_str();
        remote_name |= options
            && (["--remote-name", "--remote-name-all"].contains(&text)
                || text.starts_with('-') && !text[1..].contains('-') && text.ends_with('O'));
        if options && (text == "--output-dir" || text.starts_with("--output-dir=")) {
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
        let url = if options
            && text
                .get(..6)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("--url="))
        {
            &text[6..]
        } else {
            text
        };
        if file_url(context, index, url) {
            index += 1;
            continue;
        }
        if !options || word.role == Role::Option(OptionRole::Name) {
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
        option_target(context, index, key, value);
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

fn option_target(context: &mut Context<'_>, index: usize, key: &str, value: &str) {
    match key {
        "d" | "data" | "data-ascii" | "data-binary" | "data-urlencode" | "json" | "H"
        | "header" | "proxy-header" | "url-query" | "variable" => {
            if let Some((_, path)) = value.split_once('@').filter(|(_, path)| !path.is_empty()) {
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
}

fn file_url(context: &mut Context<'_>, index: usize, url: &str) -> bool {
    if !url
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
    {
        return false;
    }
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
    true
}
