use super::*;

pub(super) fn interpreter_code(program: &str, args: &[Word]) -> (Vec<String>, Vec<usize>) {
    let (code, value, glued) = match program {
        "python" | "python3" => ("c", "WX", true),
        "node" => ("ep", "", false),
        "bun" => ("ep", "", true),
        "ruby" => ("e", "rICEix", true),
        "perl" => ("eE", "MmIidDCFx", true),
        "php" => ("rR", "dcfz", true),
        "osascript" => ("e", "", true),
        "lua" => ("e", "l", true),
        _ => ("", "", false),
    };
    let mut found = Vec::new();
    let mut claimed = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if program == "deno" && arg == "eval" {
            claimed.push(index);
            for (offset, source) in args[index + 1..].iter().enumerate() {
                if !source.starts_with('-') {
                    found.push(source.text.clone());
                    claimed.push(index + 1 + offset);
                }
            }
            break;
        }
        let long = arg
            .strip_prefix("--")
            .map(|s| s.split_once('=').map_or((s, None), |(k, v)| (k, Some(v))));
        if let Some((key, inline)) = long
            && (["eval", "print"].contains(&key) || program == "php" && key == "run")
        {
            if let Some(source) = inline {
                found.push(source.to_owned());
            } else {
                index += 1;
                if let Some(source) = args.get(index) {
                    found.push(source.text.clone());
                    claimed.push(index);
                }
            }
        } else if arg.starts_with('-') && !arg.starts_with("--") {
            for (offset, ch) in arg.char_indices().skip(1) {
                // Perl's in-place suffix is attached only; a bare -i does not
                // consume the next word.
                if program == "perl" && ch == 'i' {
                    break;
                }
                if value.contains(ch) {
                    if offset + 1 == arg.len() {
                        index += 1;
                        claimed.push(index);
                    }
                    break;
                }
                if code.contains(ch) {
                    if offset + 1 < arg.len() && !glued {
                        continue;
                    }
                    if offset + 1 < arg.len() {
                        found.push(arg[offset + 1..].into());
                    } else {
                        index += 1;
                        if let Some(source) = args.get(index) {
                            found.push(source.text.clone());
                            claimed.push(index);
                        }
                    }
                    break;
                }
            }
        }
        index += 1;
    }
    (found, claimed)
}

pub(super) fn perl_exec_argv(args: &[Word]) -> Option<&[Word]> {
    let mut sources = Vec::new();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg.expands || arg.runtime_unknown || arg.cardinality_unknown {
            return None;
        }
        if arg == "--" {
            index += 1;
            break;
        }
        if let Some(code) = arg.strip_prefix("-e").or_else(|| arg.strip_prefix("-E")) {
            if code.is_empty() {
                index += 1;
                let code = args.get(index)?;
                if code.expands || code.runtime_unknown || code.cardinality_unknown {
                    return None;
                }
                sources.push(code.text.clone());
            } else {
                sources.push(code.to_owned());
            }
        } else if arg.starts_with('-') {
            return None;
        } else {
            break;
        }
        index += 1;
    }
    if sources.is_empty() {
        return None;
    }
    let source = sources.join("\n");
    let source = source
        .lines()
        .map(|line| line.split_once('#').map_or(line, |(code, _)| code))
        .collect::<Vec<_>>()
        .join("\n");
    let mut statements = source
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if statements.pop()?.strip_prefix("exec")?.trim() != "@ARGV" {
        return None;
    }
    if !statements.iter().all(|statement| {
        statement.strip_prefix("alarm").is_some_and(|value| {
            value.starts_with(char::is_whitespace)
                && !value.trim().is_empty()
                && value.trim().bytes().all(|byte| byte.is_ascii_digit())
        })
    }) {
        return None;
    }
    let forwarded = &args[index..];
    // Two guaranteed fields select execvp list semantics, even if later array
    // fields have unknown width. An unresolved program retains the interpreter owner.
    if forwarded.len() < 2
        || forwarded[..2].iter().any(|word| word.cardinality_unknown)
        || forwarded[0].starts_with('-')
        || forwarded[0].expands
        || forwarded[0].runtime_unknown
        || forwarded[0].globs
    {
        return None;
    }
    Some(forwarded)
}
