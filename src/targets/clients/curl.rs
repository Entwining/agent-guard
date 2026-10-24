use super::*;

pub(in crate::targets) const CURL_VALUE_LETTERS: &str = "AbcCdDeEFHKmoPQrTtuUwxXyYz";
// The options whose value is a file curl uses, such as a certificate or key.
const CURL_USE: &[&str] = &[
    "cacert",
    "capath",
    "cert",
    "key",
    "E",
    "netrc-file",
    "crlfile",
    "egd-file",
    "knownhosts",
    "proxy-cacert",
    "proxy-capath",
    "proxy-cert",
    "proxy-crlfile",
    "proxy-key",
    "random-file",
    "pubkey",
    "pinnedpubkey",
    "proxy-pinnedpubkey",
    "unix-socket",
];
// The options of curl's option table in tool_getparam.c that agents use:
// flags, which `--no-` may negate, and options that take a value.
const CURL_FLAGS: &[&str] = &[
    "anyauth",
    "append",
    "basic",
    "buffer",
    "compressed",
    "create-dirs",
    "digest",
    "disable",
    "fail",
    "fail-early",
    "fail-with-body",
    "get",
    "globoff",
    "head",
    "help",
    "http1.0",
    "http1.1",
    "http2",
    "http2-prior-knowledge",
    "http3",
    "include",
    "insecure",
    "ipv4",
    "ipv6",
    "junk-session-cookies",
    "keepalive",
    "list-only",
    "location",
    "location-trusted",
    "manual",
    "negotiate",
    "netrc",
    "netrc-optional",
    "next",
    "ntlm",
    "parallel",
    "path-as-is",
    "progress-bar",
    "progress-meter",
    "raw",
    "remote-header-name",
    "remote-name",
    "remote-name-all",
    "remote-time",
    "retry-all-errors",
    "retry-connrefused",
    "sessionid",
    "show-error",
    "show-headers",
    "silent",
    "ssl-reqd",
    "styled-output",
    "tcp-nodelay",
    "tr-encoding",
    "verbose",
    "version",
];
const CURL_FLAG_LETTERS: &str = "#012346:BGIJLMNORSVZafgijklnpqsv";
const CURL_VALUES: &[&str] = &[
    "abstract-unix-socket",
    "alt-svc",
    "aws-sigv4",
    "cacert",
    "capath",
    "cert",
    "cert-type",
    "ciphers",
    "config",
    "connect-timeout",
    "connect-to",
    "continue-at",
    "cookie",
    "cookie-jar",
    "create-file-mode",
    "crlfile",
    "data",
    "data-ascii",
    "data-binary",
    "data-raw",
    "data-urlencode",
    "dns-servers",
    "doh-url",
    "dump-header",
    "egd-file",
    "etag-compare",
    "etag-save",
    "expect100-timeout",
    "form",
    "form-string",
    "header",
    "hsts",
    "interface",
    "json",
    "keepalive-time",
    "key",
    "key-type",
    "knownhosts",
    "libcurl",
    "limit-rate",
    "local-port",
    "mail-from",
    "mail-rcpt",
    "max-filesize",
    "max-redirs",
    "max-time",
    "netrc-file",
    "noproxy",
    "oauth2-bearer",
    "output",
    "output-dir",
    "parallel-max",
    "pass",
    "pinnedpubkey",
    "proto",
    "proto-redir",
    "proxy",
    "proxy-cacert",
    "proxy-capath",
    "proxy-cert",
    "proxy-crlfile",
    "proxy-header",
    "proxy-key",
    "proxy-pinnedpubkey",
    "proxy-user",
    "pubkey",
    "quote",
    "random-file",
    "range",
    "referer",
    "request",
    "resolve",
    "retry",
    "retry-delay",
    "retry-max-time",
    "socks5",
    "socks5-hostname",
    "speed-limit",
    "speed-time",
    "ssl-sessions",
    "stderr",
    "time-cond",
    "tls-max",
    "trace",
    "trace-ascii",
    "trace-config",
    "unix-socket",
    "upload-file",
    "url",
    "url-query",
    "user",
    "user-agent",
    "variable",
    "write-out",
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
        // From runtime text that could spell an option, no option consumes
        // a word: each later word is also inspected on its own, and even a
        // later `--` may be an option's value.
        if options && context.unproven.is_none() && word.option_unknown() {
            context.unproven = Some(index);
        }
        let scan = context.unproven.is_some();
        if options && !scan && word.exact() == Some("--") {
            options = false;
            index += 1;
            continue;
        }
        let text = word.as_str();
        if !options || text == "-" || !text.starts_with('-') {
            url(context, index, text);
            index += 1;
            continue;
        }
        remote_name |= ["--remote-name", "--remote-name-all"].contains(&text)
            || !text[1..].contains('-') && text.ends_with('O');
        // An option the grammar does not know may take the next word.
        let (key, attached) = if let Some(option) = text.strip_prefix("--") {
            let (key, attached) = option
                .split_once('=')
                .map_or((option, None), |(key, value)| (key, Some(value)));
            if !CURL_VALUES.contains(&key) {
                if attached.is_some()
                    || !CURL_FLAGS.contains(&key.strip_prefix("no-").unwrap_or(key))
                {
                    context.unproven.get_or_insert(index);
                }
                index += 1;
                continue;
            }
            (key, attached)
        } else {
            match text
                .char_indices()
                .skip(1)
                .find(|(_, letter)| !CURL_FLAG_LETTERS.contains(*letter))
            {
                Some((at, letter)) if CURL_VALUE_LETTERS.contains(letter) => {
                    let attached = &text[at + letter.len_utf8()..];
                    (
                        &text[at..at + letter.len_utf8()],
                        (!attached.is_empty()).then_some(attached),
                    )
                }
                found => {
                    if found.is_some() {
                        context.unproven.get_or_insert(index);
                    }
                    index += 1;
                    continue;
                }
            }
        };
        let holder = if attached.is_some() { index } else { index + 1 };
        if holder >= context.words.len() {
            break;
        }
        let value = attached.unwrap_or_else(|| context.text(holder));
        // The value is data of its option even when it looks like one.
        context.claimed[holder] = true;
        if key == "output-dir" {
            output_dir = Some(holder);
        } else {
            option_target(context, holder, key, value);
        }
        index = if scan { index + 1 } else { holder + 1 };
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

/// The resource a URL names: `@FILE` reads a list of URLs from FILE, and a
/// file: URL reads its path.
fn url(context: &mut Context<'_>, index: usize, url: &str) {
    if let Some(path) = url.strip_prefix('@') {
        if !path.is_empty() && path != "-" {
            context.add(path, Some(index), Effect::Read, None);
        }
    } else {
        file_url(context, index, url);
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
        "url" => url(context, index, value),
        "w" | "write-out" if value.starts_with('@') && value.len() > 1 && value != "@-" => {
            context.add(&value[1..], Some(index), Effect::Read, Some(true));
        }
        "b" | "cookie" if !value.is_empty() && !value.contains('=') => {
            context.add(value, Some(index), Effect::Use, None);
        }
        key if CURL_WRITES.contains(&key) && !value.is_empty() => {
            if value != "-" {
                context.add(value, Some(index), Effect::Write, None);
            }
        }
        key if CURL_USE.contains(&key) => {
            context.add(value, Some(index), Effect::Use, None);
        }
        _ => {}
    }
}

fn file_url(context: &mut Context<'_>, index: usize, url: &str) {
    if !url
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
    {
        return;
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
}
