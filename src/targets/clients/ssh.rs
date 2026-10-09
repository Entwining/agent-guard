use super::*;

pub(super) fn ssh(program: &str, context: &mut Context<'_>) {
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
        let (effect, paths) = if letter == 'o' {
            configured_paths(value)
        } else {
            let effect = match letter {
                'i' | 'F' | 'S' => Some(Effect::Use),
                'E' => Some(Effect::Write),
                'b' if program == "sftp" && value != "-" => Some(Effect::Read),
                _ => None,
            };
            (effect, vec![value.to_owned()])
        };

        if let Some(effect) = effect {
            for path in paths {
                context.add(&path, Some(index), effect, (letter == 'o').then_some(false));
            }
        }
        index += 1;
    }
}

fn configured_paths(value: &str) -> (Option<Effect>, Vec<String>) {
    let mut paths = Vec::new();
    let mut effect = None;
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
    (effect, paths)
}
