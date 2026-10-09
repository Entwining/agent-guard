use super::*;

pub(super) const WGET_USE: &[(&str, Effect)] = &[
    ("--ca-certificate", Effect::Use),
    ("--ca-directory", Effect::Use),
    ("--certificate", Effect::Use),
    ("--private-key", Effect::Use),
    ("--crl-file", Effect::Use),
    ("--random-file", Effect::Use),
];
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

pub(super) fn wget(context: &mut Context<'_>) {
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
