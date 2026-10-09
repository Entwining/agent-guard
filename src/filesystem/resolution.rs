use super::{
    CheckError, FirmlinkTable, Probe, Protection, absolute_input, glob, lexical, links,
    literal_shell_pattern,
};

pub(super) enum Resolution {
    Public(String),
    Protected(Protection, String),
    Bound,
    InodeAlias,
    InheritedInput,
}

pub(super) fn resolve(
    path: &str,
    cwd: &str,
    home: &str,
    resolved_home: Option<&str>,
    table: &FirmlinkTable,
    probe: &mut dyn Probe,
    lexical: &mut lexical::Lexical,
) -> Result<Resolution, CheckError> {
    links::follow(
        &absolute_input(path, cwd, home),
        home,
        resolved_home,
        table,
        probe,
        lexical,
    )
}

pub(super) fn resolve_pattern(
    absolute: &str,
    homes: (&str, Option<&str>),
    patterned: bool,
    pattern: Option<&str>,
    table: &FirmlinkTable,
    probe: &mut dyn Probe,
    lexical: &mut lexical::Lexical,
) -> Result<Resolution, CheckError> {
    let (home, resolved_home) = homes;
    if patterned {
        let segments: Vec<_> = absolute.split('/').collect();
        let pattern_segments: Vec<_> = pattern.unwrap_or(absolute).split('/').collect();
        if let Some(index) = segments.iter().enumerate().position(|(index, part)| {
            if pattern.is_some() {
                pattern_segments
                    .get(index)
                    .is_some_and(|part| glob::shell_syntax(part))
            } else {
                part.contains(['*', '?', '[', '{', '$', '`'])
            }
        }) {
            let prefix = segments[..index].join("/");
            let prefix = if prefix.is_empty() { "/" } else { &prefix };
            return match links::follow(prefix, home, resolved_home, table, probe, lexical)? {
                Resolution::Public(resolved) if resolved == prefix => {
                    Ok(Resolution::Public(absolute.to_owned()))
                }
                Resolution::Public(resolved) => Ok(Resolution::Public(format!(
                    "{}/{}",
                    resolved.trim_end_matches('/'),
                    segments[index..].join("/")
                ))),
                other => Ok(other),
            };
        }
    }
    links::follow(absolute, home, resolved_home, table, probe, lexical)
}

pub(super) fn rebase_pattern(raw: &str, resolved: &str, pattern: &str) -> String {
    let old: Vec<_> = raw.split('/').collect();
    let new: Vec<_> = resolved.split('/').collect();
    let encoded: Vec<_> = pattern.split('/').collect();
    let shared = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let prefix = literal_shell_pattern(&new[..new.len() - shared].join("/"));
    format!(
        "{}/{}",
        prefix.trim_end_matches('/'),
        encoded[encoded.len() - shared..].join("/")
    )
}
