use super::Consumer;
use crate::filesystem;
use serde_json::Value;

/// Claude Code 2.1.295 `coerceInput` aliases, applied before it validates a
/// call: Grep reads a non-empty `file_path` as `path` unless a different
/// `path` is present, and Write reads a string `path` as `file_path` when
/// `file_path` is absent. Read and Edit have no path alias.
pub(super) fn claude_aliases(tool: &str, input: &mut Value) {
    let Some(fields) = input.as_object_mut() else {
        return;
    };
    let rename = match tool {
        "grep" => fields
            .get("file_path")
            .and_then(Value::as_str)
            .is_some_and(|path| {
                !path.is_empty()
                    && fields
                        .get("path")
                        .is_none_or(|current| current.as_str() == Some(path))
            })
            .then_some(("file_path", "path")),
        "write" => (fields.get("path").is_some_and(Value::is_string)
            && !fields.contains_key("file_path"))
        .then_some(("path", "file_path")),
        _ => None,
    };
    if let Some((alias, field)) = rename
        && let Some(value) = fields.remove(alias)
    {
        fields.insert(field.to_owned(), value);
    }
}

/// Rewrites a tool path the way the consumer resolves it before opening the
/// file, so the guard checks the file the tool will actually read or write.
pub(super) fn consumer_path(consumer: Consumer, raw: String) -> String {
    match consumer {
        // Claude Code trims the path with JavaScript `String.prototype.trim`.
        Consumer::Claude => raw.trim_matches(js_whitespace).to_owned(),
        Consumer::Codex => raw,
        Consumer::Pi => pi_path(&raw),
    }
}

/// The filters the consumer's Grep tool passes to ripgrep, one `--glob` each.
pub(super) fn grep_globs(consumer: Consumer, glob: String) -> Vec<String> {
    match consumer {
        // Claude Code splits on JavaScript `\s`, then on commas unless a
        // piece holds both braces, and drops empty pieces.
        Consumer::Claude => glob
            .split(js_whitespace)
            .flat_map(|piece| {
                if piece.contains('{') && piece.contains('}') {
                    vec![piece]
                } else {
                    piece.split(',').collect()
                }
            })
            .filter(|piece| !piece.is_empty())
            .map(str::to_owned)
            .collect(),
        Consumer::Codex | Consumer::Pi => vec![glob],
    }
}

/// JavaScript's `\s` and `trim` set: Unicode White_Space without U+0085,
/// plus U+FEFF.
fn js_whitespace(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

/// The absolute path the consumer opens for a path `consumer_path` rewrote.
pub(crate) fn opened_path(consumer: Consumer, path: &str, cwd: &str, home: &str) -> String {
    match consumer {
        Consumer::Claude | Consumer::Pi => filesystem::resolve_tool_path(path, cwd, home),
        // The documented Codex registration covers Bash only, so no Codex
        // tool path has an observed resolution to follow.
        Consumer::Codex => filesystem::absolute_input(path, cwd, home),
    }
}

/// Pi 1.1.0 `normalizePath` with `normalizeUnicodeSpaces` and `stripAtPrefix`:
/// Unicode spaces become ASCII spaces and one leading `@` is dropped.
fn pi_path(raw: &str) -> String {
    let spaced: String = raw
        .chars()
        .map(|c| match c {
            '\u{a0}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}' => ' ',
            other => other,
        })
        .collect();
    spaced.strip_prefix('@').unwrap_or(&spaced).to_owned()
}
