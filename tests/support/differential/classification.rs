use super::*;

// Frozen legacy observations select effective programs, not expected verdicts
// or the current parser's output, so scope classification stays independent.
pub(super) fn outside_slice(row: &Value, scope: &BTreeMap<String, Value>) -> Option<String> {
    if row["tool"] != "Bash" {
        return None;
    }
    let id = format!("{}[{}]", row["family"].as_str().unwrap(), row["index"]);
    let selected = &scope[&id];
    let programs = selected["programs"].as_array().unwrap();
    if programs.is_empty()
        || programs.iter().any(|program| {
            let name = program.as_str().unwrap();
            MODELLED.contains(&name)
                || ["python", "node", "ruby", "perl", "php", "lua"]
                    .iter()
                    .any(|base| {
                        name.strip_prefix(base).is_some_and(|suffix| {
                            !suffix.is_empty()
                                && suffix.chars().all(|c| c.is_ascii_digit() || c == '.')
                        })
                    })
        })
    {
        return None;
    }
    Some(format!(
        "unmodelled programs: {}",
        programs
            .iter()
            .map(|p| p.as_str().unwrap())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

pub(super) fn reason_partition(reason: &str) -> &'static str {
    let reason = reason.to_lowercase();
    if reason.contains("symlink") {
        "identity"
    } else if reason.contains("inline code") {
        "CodeFile"
    } else if reason.contains("hidden files") {
        "hidden_content"
    } else if reason.contains("shell syntax") {
        "syntax"
    } else if reason.contains("dumps environment") {
        "dump"
    } else if reason.contains("variable") {
        "variable"
    } else if reason.contains("app-data") {
        "AppData"
    } else if reason.contains("credential or environment file") {
        "credential_file"
    } else if reason.contains(".ssh") {
        "SSH"
    } else {
        "other_secret_owner"
    }
}

pub(super) fn go_reason_rule(reason: &str) -> &'static str {
    for (prefix, rule) in [
        ("This reads a protected", "AppData"),
        ("A scan rooted", "Broad"),
        ("This reads a credential", "File"),
        ("This inline code", "CodeFile"),
        ("A recursive search", "HiddenSearch"),
        ("This dumps", "Dump"),
        ("This prints the value", "Variable"),
        ("This prints a Git", "Token"),
        ("This extracts a password", "Keychain"),
        ("This prints a stored", "StoredSecret"),
        ("curl verbose", "Trace"),
        ("This sends", "Upload"),
        ("This reads private", "Ssh"),
        ("Grep would search private", "GrepSsh"),
        ("The agent guard cannot inspect this shell syntax", "Syntax"),
        (
            "The agent guard could not complete its symlink check",
            "Symlink",
        ),
    ] {
        if reason.starts_with(prefix) {
            return rule;
        }
    }
    assert!(reason.is_empty(), "unclassified Go reason: {reason}");
    "None"
}

const MODELLED: &[&str] = &[
    "cat",
    "head",
    "tail",
    "less",
    "more",
    "bat",
    "sort",
    "uniq",
    "cut",
    "nl",
    "base64",
    "xxd",
    "od",
    "strings",
    "rg",
    "grep",
    "ag",
    "ack",
    "fd",
    "tree",
    "ls",
    "du",
    "find",
    "tar",
    "git",
    "printf",
    "echo",
    "print",
    "set",
    "declare",
    "typeset",
    "printenv",
    "export",
    "env",
    "xargs",
    "command",
    "exec",
    "nohup",
    "timeout",
    "nice",
    "sudo",
    "doas",
    "sh",
    "bash",
    "zsh",
    "dash",
    "ksh",
    "csh",
    "tcsh",
    "eval",
    "source",
    ".",
    "python",
    "python3",
    "node",
    "bun",
    "ruby",
    "perl",
    "php",
    "osascript",
    "lua",
    "deno",
    "true",
    "false",
    ":",
    "cd",
    "unset",
    "local",
    "setopt",
    "unsetopt",
    "emulate",
];
