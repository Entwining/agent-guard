use super::lexical as lexical_state;
use super::{CheckError, Protection, glob, ssh_public};

pub fn lexical(path: &str, home: &str) -> Option<Protection> {
    lexical_pattern(path, home, true)
}

pub fn lexical_literal(path: &str, home: &str) -> Option<Protection> {
    lexical_pattern(path, home, false)
}

pub fn appdata_fragment(path: &str) -> bool {
    let path = path.to_lowercase();
    [
        "containers",
        "group containers",
        "mobile documents",
        "cloudstorage",
    ]
    .iter()
    .any(|tree| {
        let root = format!("/library/{tree}");
        path.ends_with(&root) || path.contains(&format!("{root}/"))
    })
}

/// Public App Data wording needs a tree-specific match; broader protection
/// may need the project-scope alternative. This selects prose, never permission.
pub(crate) fn appdata_reason(path: &str, home: &str, patterned: bool) -> bool {
    let library = format!("{home}/Library/").to_lowercase();
    let trees = [
        "containers",
        "group containers",
        "mobile documents",
        "cloudstorage",
    ];
    glob::alternatives(path, patterned).iter().any(|candidate| {
        let candidate = candidate.to_lowercase();
        if patterned {
            let parts: Vec<_> = candidate.split('/').collect();
            for tree in trees {
                let root = format!("{library}{tree}");
                let root_parts: Vec<_> = root.split('/').collect();
                if parts.len() > root_parts.len()
                    && root_parts.iter().enumerate().all(|(index, part)| {
                        index == 0 || parts[index] == "**" || glob::component(parts[index], part)
                    })
                {
                    return true;
                }
            }
        }
        let Some(rest) = candidate.strip_prefix(&library) else {
            return false;
        };
        trees
            .iter()
            .any(|tree| rest == *tree || rest.starts_with(&format!("{tree}/")))
            || patterned && {
                let fixed = rest
                    .find(['*', '?', '['])
                    .map_or(rest, |at| &rest[..at])
                    .trim_end_matches('/');
                !fixed.is_empty() && trees.iter().any(|tree| tree.starts_with(fixed))
            }
    })
}

pub(super) fn lexical_pattern(path: &str, home: &str, patterned: bool) -> Option<Protection> {
    lexical_pattern_mode(path, home, patterned, true)
}

pub(super) fn lexical_pattern_mode(
    path: &str,
    home: &str,
    patterned: bool,
    hidden: bool,
) -> Option<Protection> {
    match lexical_pattern_checked(path, home, patterned, hidden, None) {
        Ok(result) => result,
        Err(_) => unreachable!("lexical matching without a deadline cannot expire"),
    }
}

pub(super) fn lexical_pattern_checked(
    path: &str,
    home: &str,
    patterned: bool,
    hidden: bool,
    deadline: Option<std::time::Instant>,
) -> Result<Option<Protection>, CheckError> {
    lexical_state::Lexical::new(deadline).check(path, home, patterned, hidden)
}

pub fn broad_root(path: &str, home: &str, patterned: bool) -> bool {
    match broad_root_checked(path, home, patterned, None) {
        Ok(result) => result,
        Err(_) => unreachable!("broad matching without a deadline cannot expire"),
    }
}

pub(crate) fn broad_root_checked(
    path: &str,
    home: &str,
    patterned: bool,
    deadline: Option<std::time::Instant>,
) -> Result<bool, CheckError> {
    lexical_state::Lexical::new(deadline).broad(path, home, patterned)
}

pub(super) fn broad_prepared(
    path: &str,
    domain: &lexical_state::Domain,
    patterned: bool,
    matcher: &mut glob::Matcher,
    deadline: Option<std::time::Instant>,
) -> Result<bool, CheckError> {
    let path = path.to_lowercase();
    let home = &domain.home;
    let (prefix, rest) = if patterned {
        glob::literal_prefix(&path)
    } else {
        (path.clone(), "")
    };
    let literal = rest.is_empty()
        && (prefix == "/"
            || &prefix == home
            || prefix == domain.library
            || home.starts_with(&format!("{}/", prefix.trim_end_matches('/'))));
    if literal || !patterned {
        return Ok(literal);
    }
    // A recursive glob can reach protected descendants without matching one
    // of the finite witnesses, so check its literal root independently.
    for pattern in glob::alternatives(&path, true) {
        crate::check_deadline(deadline)?;
        let (prefix, _) = glob::literal_prefix(&pattern);
        let prefix = prefix.trim_end_matches('/');
        for candidate in &domain.broad_witnesses {
            if matcher.path_checked(&pattern, candidate, deadline)? {
                return Ok(true);
            }
        }
        if glob::recursive_wildcard(&pattern)
            && (prefix == home
                || prefix == domain.library
                || home.starts_with(&format!("{prefix}/")))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn lexical_candidate(
    path: &str,
    domain: &lexical_state::Domain,
    patterned: bool,
    hidden: bool,
    matcher: &mut glob::Matcher,
) -> Option<Protection> {
    // Brace expansion can leave only encoded literals. Root and filename
    // comparisons then need the same text the matcher would compare.
    let literal = patterned
        .then(|| glob::literal_prefix(path))
        .and_then(|(prefix, rest)| rest.is_empty().then_some(prefix));
    let spelling = literal.as_deref().unwrap_or(path);
    let patterned = patterned && literal.is_none();
    let state = lexical_state::LiteralState::path(spelling, &domain.library_parts);
    if !patterned {
        return state.protection();
    }
    let path = spelling.to_lowercase();
    let parts: Vec<_> = path.split('/').collect();
    if state.appdata
        || domain.roots.iter().any(|root| {
            matcher.path(&path, &root.path)
                || parts.len() > root.parts.len()
                    && root.parts.iter().enumerate().all(|(index, part)| {
                        parts[index] == "**" || matcher.component(parts[index], part)
                    })
        })
    {
        return Some(Protection::AppData);
    }
    if let Some(index) = parts.iter().position(|part| {
        *part == ".ssh" || part.starts_with('.') && matcher.component(part, ".ssh")
    }) {
        let original: Vec<_> = spelling.split('/').collect();
        let tail = &original[index + 1..];
        if !tail.is_empty() && (tail.len() != 1 || !ssh_public(tail[0])) {
            return Some(Protection::SshPrivate);
        }
    }
    let base = parts.last().copied().unwrap_or("");
    if base == ".env.example" || base == ".env.age" {
        return None;
    }
    if state.environment
        || parts.iter().any(|part| {
            !part.trim_matches(['*', '?']).is_empty()
                && (matcher.component(part, ".env") || matcher.component(part, ".env.x"))
        })
    {
        return Some(Protection::Environment);
    }
    if state.credential
        || parts.iter().any(|part| {
            part.contains('\\') && {
                let (literal, rest) = glob::literal_prefix(part);
                rest.is_empty() && literal == "private-keys-v1.d"
            }
        })
    {
        return Some(Protection::Credential);
    }
    if base.trim_matches(['*', '?']).is_empty() {
        let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
        if [".ssh", ".aws", ".gnupg"].contains(&parent.rsplit('/').next().unwrap_or(""))
            || lexical_state::catalog()
                .directory_suffixes
                .iter()
                .any(|suffix| parent.ends_with(suffix))
        {
            return Some(Protection::Credential);
        }
    }
    for listed in &lexical_state::catalog().sensitive {
        if matcher.path(listed.pattern, &path) {
            return Some(Protection::Credential);
        }
        let tail = &listed.tail;
        if tail.len() > parts.len()
            || tail[tail.len() - 1].trim_matches('*').is_empty()
            || base.trim_matches(['*', '?']).is_empty()
        {
            continue;
        }
        let offset = parts.len() - tail.len();
        if tail[..tail.len() - 1].iter().enumerate().all(|(index, _)| {
            matcher.visible_component(parts[offset + index], &listed.witnesses[index], hidden)
        }) && if tail.len() > 1 {
            // Recursive directory wildcards alone do not identify a credential.
            parts[offset..]
                .iter()
                .zip(tail)
                .any(|(pattern, protected)| matcher.identifies_component(pattern, protected))
                && glob::visible_intersects(base, tail[tail.len() - 1], hidden)
        } else {
            matcher.visible_component(base, &listed.witnesses[0], hidden)
        } {
            return Some(Protection::Credential);
        }
    }
    None
}

pub(super) const SENSITIVE: &[&str] = &[
    "**/.npmrc",
    "**/.zprofile*",
    "**/.zsh_history*",
    "**/*.pem",
    "**/*.key",
    "**/auth.json*",
    "**/.credentials.json*",
    "**/.aws/credentials*",
    "**/.netrc",
    "**/.git-credentials",
    "**/.docker/config.json",
    "**/.kube/config",
    "**/.pypirc",
    "**/.pgpass",
    "**/.cargo/credentials*",
    "**/.config/gh/hosts.yml",
    "**/private-keys-v1.d",
    "**/private-keys-v1.d/**",
];

fn listed_directories() -> impl Iterator<Item = &'static str> {
    lexical_state::catalog().directories.iter().copied()
}

pub(super) fn sensitive_root(path: &str, home: &str) -> bool {
    let path = path.to_lowercase();
    let home = home.to_lowercase();
    [".aws", ".gnupg"].contains(&path.rsplit('/').next().unwrap_or(""))
        || listed_directories().any(|dir| {
            path.ends_with(&format!("/{dir}"))
                || dir
                    .match_indices('/')
                    .any(|(at, _)| path == format!("{home}/{}", &dir[..at]))
        })
}
