use super::lexical as lexical_state;
use super::stores::{self, Location};
use super::{CheckError, Protection, glob, ssh_public};

pub fn lexical(path: &str, home: &str) -> Option<Protection> {
    lexical_pattern(path, home, true)
}

pub fn lexical_literal(path: &str, home: &str) -> Option<Protection> {
    lexical_pattern(path, home, false)
}

pub fn appdata_fragment(path: &str) -> bool {
    let path = super::fold(path);
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
pub(crate) fn appdata_reason(
    path: &str,
    home: &str,
    patterned: bool,
) -> Result<bool, crate::CheckError> {
    let library = super::fold(&format!("{home}/Library/"));
    let trees = [
        "containers",
        "group containers",
        "mobile documents",
        "cloudstorage",
    ];
    Ok(glob::alternatives(path, patterned)?
        .iter()
        .any(|candidate| {
            let candidate = if patterned {
                glob::fold_pattern(candidate)
            } else {
                super::fold(candidate)
            };
            if patterned {
                let parts: Vec<_> = candidate.split('/').collect();
                for tree in trees {
                    let root = format!("{library}{tree}");
                    let root_parts: Vec<_> = root.split('/').collect();
                    if parts.len() > root_parts.len()
                        && root_parts.iter().enumerate().all(|(index, part)| {
                            index == 0
                                || parts[index] == "**"
                                || glob::component(parts[index], part)
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
        }))
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
    lexical_state::Lexical::new(deadline, &[]).check(path, home, patterned, hidden)
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
    lexical_state::Lexical::new(deadline, &[]).broad(path, home, patterned)
}

pub(super) fn broad_prepared(
    path: &str,
    domain: &lexical_state::Domain,
    patterned: bool,
    matcher: &mut glob::Matcher,
    deadline: Option<std::time::Instant>,
) -> Result<bool, CheckError> {
    let lowered = super::fold(path);
    let home = &domain.home;
    let (prefix, rest) = if patterned {
        glob::literal_prefix(&lowered)
    } else {
        (lowered.clone(), "")
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
    for pattern in glob::alternatives(path, true)? {
        crate::check_deadline(deadline)?;
        let pattern = glob::fold_pattern(&pattern);
        let (prefix, _) = glob::literal_prefix(&pattern);
        let prefix = prefix.trim_end_matches('/');
        for candidate in &domain.broad_witnesses {
            if matcher.path_checked(&pattern, candidate, true, deadline)? {
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

/// Which names a pattern's wildcards reach below its fixed directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reach {
    /// Wildcards skip dot names.
    Visible,
    /// Wildcards match dot names, and a generic file name identifies a
    /// credential only where the pattern spells its directory.
    Hidden,
    /// `**` also descends through every hidden directory, as a hidden walk
    /// narrowed by name filters does, so a generic file name identifies a
    /// credential at any depth.
    Walk,
}

pub(super) fn lexical_candidate(
    path: &str,
    domain: &lexical_state::Domain,
    locations: &[Location],
    patterned: bool,
    reach: Reach,
    matcher: &mut glob::Matcher,
) -> Option<Protection> {
    let hidden = reach != Reach::Visible;
    let stores = || domain.stores.iter().chain(locations);
    // Quoting and brace expansion escape literal text, which root, directory
    // and filename comparisons need as the matcher reads it.
    let (spelling, patterned) = if patterned {
        match glob::literal_prefix(path) {
            (literal, "") => (std::borrow::Cow::Owned(literal), false),
            _ => (std::borrow::Cow::Owned(glob::minimal_escapes(path)), true),
        }
    } else {
        (std::borrow::Cow::Borrowed(path), false)
    };
    let spelling = spelling.as_ref();
    let state = lexical_state::LiteralState::path(spelling, &domain.library_parts);
    if !patterned {
        return state.protection().or_else(|| {
            stores::holds(stores(), &super::fold(spelling)).then_some(Protection::Credential)
        });
    }
    let path = glob::fold_pattern(spelling);
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
        || parts
            .iter()
            .any(|part| selects_environment(part, hidden, &domain.library_parts, matcher))
    {
        return Some(Protection::Environment);
    }
    if state.credential {
        return Some(Protection::Credential);
    }
    if stores::selects(stores(), &path, hidden, matcher) {
        return Some(Protection::Credential);
    }
    if base.trim_matches(['*', '?']).is_empty()
        && sensitive_root(path.rsplit_once('/').map_or("", |(parent, _)| parent))
    {
        return Some(Protection::Credential);
    }
    for listed in &lexical_state::catalog().sensitive {
        if matcher.path(listed.pattern, &path) {
            return Some(Protection::Credential);
        }
        let tail = &listed.tail;
        if tail[tail.len() - 1].trim_matches('*').is_empty()
            || base.trim_matches(['*', '?']).is_empty()
        {
            continue;
        }
        let last = tail[tail.len() - 1];
        if if reach == Reach::Walk {
            // A walk reads each file under its stored name, so selecting the
            // listed name is enough; a variant that the entry's trailing `*`
            // covers still needs a pattern that identifies it.
            (glob::visible_intersects(base, last.trim_end_matches('*'), true)
                || identifies(base, listed, hidden, matcher))
                && (tail.len() == 1
                    || spells_listed_directory(&parts[..parts.len() - 1], listed, reach, matcher))
        } else if tail.len() > 1 {
            glob::visible_intersects(base, last, hidden)
                && spells_listed_directory(&parts[..parts.len() - 1], listed, reach, matcher)
        } else {
            identifies(base, listed, hidden, matcher)
        } {
            return Some(Protection::Credential);
        }
    }
    None
}

fn identifies(
    base: &str,
    listed: &lexical_state::Sensitive,
    hidden: bool,
    matcher: &mut glob::Matcher,
) -> bool {
    let last = listed.tail[listed.tail.len() - 1];
    // A witness fills a trailing `*` with text, which `*auth.json` misses
    // although it selects `auth.json` itself.
    matcher.visible_component(base, &listed.witnesses[listed.tail.len() - 1], hidden)
        || matcher.identifies_component(base, last) && glob::visible_intersects(base, last, hidden)
}

fn selects_environment(
    part: &str,
    hidden: bool,
    library: &[String],
    matcher: &mut glob::Matcher,
) -> bool {
    !part.trim_matches(['*', '?']).is_empty()
        && (matcher.component(part, ".env")
            || matcher.component(part, ".env.x")
            // A search filter's wildcards match dot names and may be empty,
            // so `*.env.local` and `.env*.local` select `.env.local` itself.
            || hidden && {
                let name = part.replace('*', "");
                lexical_state::LiteralState::path(&name, library).protection()
                    == Some(Protection::Environment)
                    && matcher.component(part, &name)
            })
}

/// Whether pattern directories can end in a catalog entry's directories, with
/// `**` spanning any number of them, while a component spells one of them or
/// a walk's `**` passes through them.
/// Generic names such as `config.json` identify a credential only with their
/// directory: a positive search glob only narrows its root, so
/// `*/config.json` reaches nothing the root alone does not. A dot-leading
/// component spells a hidden directory, as in `.*/*/config.yml`.
fn spells_listed_directory(
    pattern: &[&str],
    entry: &lexical_state::Sensitive,
    reach: Reach,
    matcher: &mut glob::Matcher,
) -> bool {
    let hidden = reach != Reach::Visible;
    // States are unmatched pattern and entry components, matched from the
    // end, and whether a component spelled a directory so far. Components
    // left before the match fall under the entry's leading `**/`.
    let mut seen = std::collections::BTreeSet::new();
    let mut pending = vec![(pattern.len(), entry.tail.len() - 1, false)];
    while let Some(state) = pending.pop() {
        if !seen.insert(state) {
            continue;
        }
        let (unmatched, remaining, spelled) = state;
        if remaining == 0 {
            if spelled {
                return true;
            }
            continue;
        }
        let Some(&part) = unmatched.checked_sub(1).map(|at| &pattern[at]) else {
            continue;
        };
        let protected = entry.tail[remaining - 1];
        if part == "**" {
            pending.push((unmatched - 1, remaining, spelled));
            if hidden || !protected.starts_with('.') {
                pending.push((unmatched, remaining - 1, spelled || reach == Reach::Walk));
            }
        } else if matcher.visible_component(part, &entry.witnesses[remaining - 1], hidden) {
            let spells = protected.starts_with('.') && part.starts_with('.')
                || matcher.identifies_component(part, protected);
            pending.push((unmatched - 1, remaining - 1, spelled || spells));
        }
    }
    false
}

pub(super) const SENSITIVE: &[&str] = &[
    "**/.npmrc",
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
    // Cargo's current stored name, which a walk's `*.toml` bound selects
    // although it does not identify the `credentials*` variants.
    "**/.cargo/credentials.toml",
    "**/.config/gh/hosts.yml",
    "**/.config/glab-cli/config.yml",
    // A bare `glab-cli` parent would let a project search for `config.yml`
    // match it.
    "**/application support/glab-cli/config.yml",
    "**/private-keys-v1.d",
    "**/private-keys-v1.d/**",
];

/// Cargo reads `.cargo/config.toml` from the working directory and its
/// ancestors, but credentials only from its home, so a `.cargo` directory is
/// not a credential directory by name; `stores` locates the home, and the
/// file names above still identify credentials in any `.cargo`.
pub(super) const CARGO_STORE: &str = ".cargo";

/// A credential directory by name, wherever it is.
pub(super) fn sensitive_root(path: &str) -> bool {
    let path = super::fold(path);
    [".aws", ".gnupg"].contains(&path.rsplit('/').next().unwrap_or(""))
        || lexical_state::catalog()
            .directory_suffixes
            .iter()
            .any(|suffix| path.ends_with(suffix))
}
