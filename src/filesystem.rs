//! Lexical checks precede identity and public search file-kind probes.

mod glob;
mod lexical;
mod links;
#[expect(
    clippy::disallowed_methods,
    reason = "Raw filesystem identity calls are confined to this probe; callers own lexical preflight."
)]
mod probe;

pub use links::FirmlinkTable;
pub use probe::DiskProbe;
pub(crate) use probe::canonicalize_home;

use crate::{CheckError, CheckErrorKind};
use std::{
    io,
    path::{Component, Path, PathBuf},
};

pub trait Probe {
    fn read_link(&mut self, path: &Path) -> io::Result<Option<PathBuf>>;
    fn stat(&mut self, path: &Path) -> io::Result<Option<Metadata>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metadata {
    pub device: u64,
    pub inode: u64,
    pub kind: FileKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    File,
    Directory,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protection {
    AppData,
    Credential,
    SshPrivate,
    Environment,
}

impl Protection {
    pub fn effect(self) -> &'static str {
        match self {
            Self::AppData => "read protected App Data",
            Self::SshPrivate => "extract protected private-key contents",
            Self::Credential => "extract protected credential-file contents",
            Self::Environment => "extract protected environment-file contents",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    Public(String),
    Protected(Protection),
    Bound,
    InodeAlias,
    InheritedInput,
}

pub fn normalize(path: &str, cwd: &str, home: &str) -> String {
    let joined = absolute_input(path, cwd, home);
    let mut clean = PathBuf::new();
    for part in Path::new(&joined).components() {
        match part {
            Component::ParentDir => {
                clean.pop();
            }
            Component::CurDir => {}
            part => clean.push(part.as_os_str()),
        }
    }
    unfirmlink(strip_path_aliases(clean.to_str().unwrap_or("")))
}

pub(crate) fn absolute_input(path: &str, cwd: &str, home: &str) -> String {
    let expanded = if let Some(tail) = path.strip_prefix("~/") {
        format!("{home}/{tail}")
    } else if path == "~" {
        home.to_owned()
    } else {
        path.to_owned()
    };
    let expanded = strip_path_aliases(strip_file_url(&expanded));
    if expanded.starts_with('/') {
        expanded.to_owned()
    } else {
        format!("{cwd}/{expanded}")
    }
}

pub(crate) fn shell_pattern(text: &str, quoted: &[std::ops::Range<usize>]) -> String {
    glob::shell_pattern(text, quoted)
}

pub(crate) fn strip_path_aliases(mut path: &str) -> &str {
    loop {
        if let Some(rest) = path.strip_prefix("/.nofollow")
            && rest.starts_with('/')
        {
            path = rest;
        } else if let Some(rest) = path.strip_prefix("/.resolve/")
            && let Some((device, _)) = rest.split_once('/')
            && !device.is_empty()
            && device.bytes().all(|byte| byte.is_ascii_digit())
        {
            path = &rest[device.len()..];
        } else {
            return path;
        }
    }
}

pub(crate) fn literal_shell_pattern(text: &str) -> String {
    shell_pattern(text, &std::iter::once(0..text.len()).collect::<Vec<_>>())
}

pub(crate) fn descriptor_path(path: &str) -> bool {
    let path = strip_path_aliases(path);
    path.strip_prefix("/dev/fd/").is_some_and(|tail| {
        let number = tail.split('/').next().unwrap_or("");
        !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
    })
}

pub(crate) fn absolute_pattern(path: &str, cwd: &str) -> String {
    if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("{}/{path}", literal_shell_pattern(cwd))
    }
}

pub(crate) fn parameter_pattern_matches(pattern: &str, value: &str) -> bool {
    glob::component(pattern, value)
}

pub(crate) fn strip_file_url(path: &str) -> &str {
    if path
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
    {
        &path[7..]
    } else {
        path
    }
}

fn unfirmlink(path: &str) -> String {
    unfirmlink_path(path).to_owned()
}

fn unfirmlink_path(path: &str) -> &str {
    let prefix = "/system/volumes/data";
    if path.eq_ignore_ascii_case(prefix) {
        "/"
    } else if path
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        && path.as_bytes().get(prefix.len()) == Some(&b'/')
    {
        &path[prefix.len()..]
    } else {
        path
    }
}

pub fn lexical(path: &str, home: &str) -> Option<Protection> {
    lexical_pattern(path, home, true)
}

pub fn expand_home(path: &str, home: &str, user: Option<&str>) -> String {
    for prefix in ["~".to_owned(), format!("~{}", user.unwrap_or("unknown"))] {
        if path == prefix || path.starts_with(&format!("{prefix}/")) {
            return format!("{home}{}", &path[prefix.len()..]);
        }
    }
    if let Some((name, tail)) = path.strip_prefix('~').and_then(|tail| tail.split_once('/'))
        && !name.is_empty()
        && name != user.unwrap_or("unknown")
    {
        let candidate = format!("{home}/{tail}");
        if lexical_literal(&candidate, home).is_some() {
            return candidate;
        }
    }
    path.to_owned()
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

fn lexical_pattern(path: &str, home: &str, patterned: bool) -> Option<Protection> {
    lexical_pattern_mode(path, home, patterned, true)
}

fn lexical_pattern_mode(
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

fn lexical_pattern_checked(
    path: &str,
    home: &str,
    patterned: bool,
    hidden: bool,
    deadline: Option<std::time::Instant>,
) -> Result<Option<Protection>, CheckError> {
    lexical::Lexical::new(deadline).check(path, home, patterned, hidden)
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
    lexical::Lexical::new(deadline).broad(path, home, patterned)
}

fn broad_prepared(
    path: &str,
    domain: &lexical::Domain,
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

fn lexical_candidate(
    path: &str,
    domain: &lexical::Domain,
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
    let state = lexical::LiteralState::path(spelling, &domain.library_parts);
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
            || lexical::catalog()
                .directory_suffixes
                .iter()
                .any(|suffix| parent.ends_with(suffix))
        {
            return Some(Protection::Credential);
        }
    }
    for listed in &lexical::catalog().sensitive {
        if matcher.path(listed.pattern, &path) {
            return Some(Protection::Credential);
        }
        let tail = &listed.tail;
        if tail.len() > parts.len() || tail[tail.len() - 1].trim_matches('*').is_empty() {
            continue;
        }
        let offset = parts.len() - tail.len();
        if tail[..tail.len() - 1].iter().enumerate().all(|(index, _)| {
            matcher.visible_component(parts[offset + index], &listed.witnesses[index], hidden)
        }) && if tail.len() > 1 || tail[0].starts_with('*') {
            glob::visible_intersects(base, tail[tail.len() - 1], hidden)
        } else {
            matcher.visible_component(base, &listed.witnesses[0], hidden)
        } {
            return Some(Protection::Credential);
        }
    }
    None
}

const SENSITIVE: &[&str] = &[
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
    lexical::catalog().directories.iter().copied()
}

fn sensitive_root(path: &str, home: &str) -> bool {
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

pub fn identify(
    path: &str,
    cwd: &str,
    home: &str,
    probe: &mut dyn Probe,
) -> Result<Identity, CheckError> {
    identify_scope(path, cwd, home, false, probe)
}

pub fn identify_scope(
    path: &str,
    cwd: &str,
    home: &str,
    search: bool,
    probe: &mut dyn Probe,
) -> Result<Identity, CheckError> {
    identify_target(
        path,
        cwd,
        home,
        search,
        true,
        crate::record::Effect::Read,
        probe,
    )
}

pub fn identify_target(
    path: &str,
    cwd: &str,
    home: &str,
    search: bool,
    patterned: bool,
    effect: crate::record::Effect,
    probe: &mut dyn Probe,
) -> Result<Identity, CheckError> {
    let table = FirmlinkTable::load()?;
    let mut resolver = Resolver::new(home, &table);
    let mut target = crate::record::Target::new(
        absolute_input(path, cwd, home),
        effect,
        if search {
            crate::record::Walk::Visible
        } else {
            crate::record::Walk::None
        },
        crate::record::Via::Operand,
    );
    target.glob = patterned;
    target.search = search;
    resolver.target(&mut target, cwd, probe)
}

pub(crate) struct Resolver<'a> {
    home: &'a str,
    table: &'a FirmlinkTable,
    deadline: Option<std::time::Instant>,
    resolved_home: Option<Identity>,
    lexical: lexical::Lexical,
    targets: std::collections::BTreeMap<TargetIdentity, (Identity, String, String, Option<String>)>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct TargetIdentity {
    pattern: Option<String>,
    unresolved: String,
    cwd: String,
    effect: crate::record::Effect,
    walk: crate::record::Walk,
    via: crate::record::Via,
    glob: bool,
    glob_hidden: bool,
    expands: bool,
    runtime_unknown: bool,
    search: bool,
}

impl<'a> Resolver<'a> {
    pub(crate) fn new(home: &'a str, table: &'a FirmlinkTable) -> Self {
        Self::with_deadline(home, table, None)
    }
    pub(crate) fn with_deadline(
        home: &'a str,
        table: &'a FirmlinkTable,
        deadline: Option<std::time::Instant>,
    ) -> Self {
        Self {
            home,
            table,
            deadline,
            resolved_home: None,
            lexical: lexical::Lexical::new(deadline),
            targets: std::collections::BTreeMap::new(),
        }
    }

    pub(crate) fn home(&mut self, probe: &mut dyn Probe) -> Result<Identity, CheckError> {
        crate::check_deadline(self.deadline)?;
        if let Some(home) = &self.resolved_home {
            return Ok(home.clone());
        }
        let home = match resolve(
            self.home,
            self.home,
            self.home,
            None,
            self.table,
            probe,
            &mut self.lexical,
        )? {
            Resolution::Public(path) => Identity::Public(normalize(&path, self.home, self.home)),
            Resolution::Protected(kind, _) => Identity::Protected(kind),
            Resolution::Bound => Identity::Bound,
            Resolution::InodeAlias => Identity::InodeAlias,
            Resolution::InheritedInput => Identity::InheritedInput,
        };
        self.resolved_home = Some(home.clone());
        Ok(home)
    }

    pub(crate) fn broad(
        &mut self,
        path: &str,
        home: &str,
        patterned: bool,
    ) -> Result<bool, CheckError> {
        self.lexical.broad(path, home, patterned)
    }

    pub(crate) fn may_traverse(
        &mut self,
        target: &crate::record::Target,
        resolved_home: &str,
        probe: &mut dyn Probe,
    ) -> Result<bool, CheckError> {
        if target.glob || target.expands || target.runtime_unknown {
            return Ok(true);
        }
        let kind = checked_stat(
            &target.path,
            self.home,
            resolved_home,
            probe,
            &mut self.lexical,
        )?
        .map(|metadata| metadata.kind);
        Ok(matches!(kind, None | Some(FileKind::Directory)))
    }

    pub(crate) fn credential(
        &mut self,
        path: &str,
        home: &str,
        patterned: bool,
    ) -> Result<Option<Protection>, CheckError> {
        Ok(self
            .lexical
            .check(path, home, patterned, true)?
            .filter(|kind| *kind != Protection::AppData)
            .or_else(|| {
                (!patterned && sensitive_root(path, home)).then_some(Protection::Credential)
            }))
    }

    pub(crate) fn target(
        &mut self,
        target: &mut crate::record::Target,
        cwd: &str,
        probe: &mut dyn Probe,
    ) -> Result<Identity, CheckError> {
        crate::check_deadline(self.deadline)?;
        let key = TargetIdentity {
            pattern: target.pattern.clone(),
            unresolved: target.unresolved.clone(),
            cwd: cwd.into(),
            effect: target.effect,
            walk: target.walk,
            via: target.via,
            glob: target.glob,
            glob_hidden: target.glob_hidden,
            expands: target.expands,
            runtime_unknown: target.runtime_unknown,
            search: target.search,
        };
        // One preflight shares one metadata observation. Roles and pattern
        // domains stay in the key, and a resolved alias must update both fields.
        if let Some((identity, path, unresolved, pattern)) = self.targets.get(&key) {
            target.path = path.clone();
            target.unresolved = unresolved.clone();
            target.pattern = pattern.clone();
            return Ok(identity.clone());
        }
        let identity = self.resolve_target(target, cwd, probe)?;
        self.targets.insert(
            key,
            (
                identity.clone(),
                target.path.clone(),
                target.unresolved.clone(),
                target.pattern.clone(),
            ),
        );
        Ok(identity)
    }

    fn resolve_target(
        &mut self,
        target: &mut crate::record::Target,
        cwd: &str,
        probe: &mut dyn Probe,
    ) -> Result<Identity, CheckError> {
        use crate::record::{Effect, Via, Walk};
        let raw = absolute_input(&target.unresolved, cwd, self.home);
        let path = normalize(&raw, cwd, self.home);
        if descriptor_path(&path) {
            return Ok(Identity::InheritedInput);
        }
        if path == "/.vol" || path.starts_with("/.vol/") {
            return Ok(Identity::InodeAlias);
        }
        if path.ends_with("/.ssh") {
            return Ok(Identity::Protected(Protection::SshPrivate));
        }
        if let Some(kind) = self.lexical.check(
            &target.pattern_path(),
            self.home,
            target.glob,
            target.glob_hidden,
        )? && !(target.glob
            && kind == Protection::Credential
            && !matches!(target.effect, Effect::Read | Effect::Change))
        {
            return Ok(Identity::Protected(kind));
        }
        if matches!(target.effect, Effect::Read | Effect::Change)
            && !target.glob
            && (target.via != Via::Cwd || target.search)
            && sensitive_root(&path, self.home)
        {
            return Ok(Identity::Protected(Protection::Credential));
        }
        if target.effect == Effect::Name && !target.glob || target.via == Via::Tool && target.glob {
            return Ok(Identity::Public(path));
        }
        let resolved_home = match self.home(probe)? {
            Identity::Public(path) => path,
            other => return Ok(other),
        };
        let resolved = match resolve_pattern(
            &raw,
            (self.home, Some(&resolved_home)),
            target.glob || target.expands,
            target.pattern.as_deref(),
            self.table,
            probe,
            &mut self.lexical,
        )? {
            Resolution::Public(resolved) => {
                if resolved != raw {
                    if let Some(pattern) = &target.pattern {
                        target.pattern = Some(rebase_pattern(&raw, &resolved, pattern));
                    }
                    target.path = resolved.clone();
                    target.unresolved = resolved.clone();
                    resolved
                } else {
                    path.clone()
                }
            }
            Resolution::Protected(kind, resolved) => {
                target.path = resolved.clone();
                target.unresolved = resolved;
                return Ok(Identity::Protected(kind));
            }
            Resolution::Bound => return Ok(Identity::Bound),
            Resolution::InodeAlias => return Ok(Identity::InodeAlias),
            Resolution::InheritedInput => return Ok(Identity::InheritedInput),
        };
        if let Some(kind) = self
            .lexical
            .check(
                &target.pattern_path(),
                self.home,
                target.glob,
                target.glob_hidden,
            )?
            .or(self.lexical.check(
                &target.pattern_path(),
                &resolved_home,
                target.glob,
                target.glob_hidden,
            )?)
        {
            return Ok(Identity::Protected(kind));
        }
        if matches!(target.effect, Effect::Read | Effect::Change)
            && !target.glob
            && (target.via != Via::Cwd || target.search)
            && sensitive_root(&resolved, &resolved_home)
        {
            return Ok(Identity::Protected(Protection::Credential));
        }
        // The broad-root owner wins before subordinate SSH metadata comparisons.
        let search = target.walk != Walk::None;
        if (search || target.glob)
            && self
                .lexical
                .broad(&target.pattern_path(), &resolved_home, target.glob)?
        {
            return Ok(Identity::Public(resolved));
        }
        if matches!(
            target.effect,
            Effect::Read | Effect::Write | Effect::Change | Effect::List
        ) {
            match self.ssh_denied(&path, &resolved, cwd, &resolved_home, target.search, probe)? {
                Some(true) => return Ok(Identity::Protected(Protection::SshPrivate)),
                None => return Ok(Identity::Bound),
                Some(false) => {}
            }
        }
        Ok(Identity::Public(resolved))
    }
}

pub(crate) fn literal_glob_root(path: &str) -> String {
    glob::escape_literal(path)
}

#[cfg(test)]
mod pattern_roots {
    #[test]
    fn equal_cooked_paths_with_different_quotes_keep_distinct_identities() {
        struct NoLinks;
        impl super::Probe for NoLinks {
            fn read_link(
                &mut self,
                _: &std::path::Path,
            ) -> std::io::Result<Option<std::path::PathBuf>> {
                Ok(None)
            }
            fn stat(&mut self, _: &std::path::Path) -> std::io::Result<Option<super::Metadata>> {
                Ok(None)
            }
        }
        let packet: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/rust-batch17e-patterns.json"
        ))
        .unwrap();
        let rows = packet["rows"].as_array().unwrap();
        let targets: Vec<_> = ["same-cooked-quoted-brace", "same-cooked-unquoted-brace"]
            .iter()
            .map(|id| {
                let row = rows.iter().find(|row| row["id"] == *id).unwrap();
                let observation = crate::shell::observe(
                    row["input"]["command"].as_str().unwrap(),
                    crate::shell::Arm::Brush,
                    "/h",
                    "/p",
                    true,
                )
                .unwrap();
                crate::record::Target::from_word(
                    &observation.script.commands.last().unwrap().argv[1],
                    "/p",
                    crate::record::HostFacts {
                        home: "/h",
                        user: None,
                    },
                    crate::record::Effect::Read,
                    crate::record::Walk::None,
                )
            })
            .collect();
        assert_eq!(targets[0].unresolved, targets[1].unresolved);
        for order in [[0, 1], [1, 0]] {
            let table = super::FirmlinkTable::from_text("");
            let mut resolver = super::Resolver::new("/h", &table);
            for index in order {
                let identity = resolver
                    .target(&mut targets[index].clone(), "/p", &mut NoLinks)
                    .unwrap();
                assert_eq!(
                    matches!(
                        identity,
                        super::Identity::Protected(super::Protection::Environment)
                    ),
                    index == 1
                );
            }
        }
    }
    #[test]
    fn quoted_pattern_width_does_not_multiply_local_candidates() {
        for width in [2, 4, 8, 16] {
            for depth in [1, 2, 4, 8] {
                let groups = format!("({})", vec!["public"; width].join("|"));
                let source = format!("cat 'public{}'*.txt", groups.repeat(depth));
                let observation = crate::shell::observe(
                    &source,
                    crate::shell::Arm::Brush,
                    "/synthetic/home",
                    "/synthetic/project",
                    true,
                )
                .unwrap();
                let word = &observation.script.commands.last().unwrap().argv[1];
                let target = crate::record::Target::from_word(
                    word,
                    "/synthetic/project",
                    crate::record::HostFacts {
                        home: "/synthetic/home",
                        user: None,
                    },
                    crate::record::Effect::Read,
                    crate::record::Walk::None,
                );
                assert!(target.glob);
                let pattern = target.pattern_path();
                let candidates = super::glob::alternatives(&pattern, true);
                assert_eq!(candidates.len(), 1, "width={width}, depth={depth}");
                let mut matcher = super::glob::Matcher::default();
                for candidate in candidates {
                    assert_eq!(
                        super::lexical_candidate(
                            &candidate,
                            &super::lexical::Domain::new("/synthetic/home"),
                            true,
                            false,
                            &mut matcher
                        ),
                        None
                    );
                }
                assert!(
                    matcher.component_queries <= 256,
                    "width={width}, depth={depth}, queries={}",
                    matcher.component_queries
                );
                println!(
                    "quoted width={width}, depth={depth}, queries={}",
                    matcher.component_queries
                );
            }
        }
    }
    #[test]
    fn repeated_pattern_components_share_match_work() {
        for size in [8, 16, 32] {
            let mut matcher = super::glob::Matcher::default();
            let mut first = None;
            for _ in 0..size {
                assert_eq!(
                    super::lexical_candidate(
                        "/h/project/public[a-z]/nested/public.json",
                        &super::lexical::Domain::new("/h"),
                        true,
                        false,
                        &mut matcher
                    ),
                    None
                );
                assert_eq!(
                    matcher.component_evaluations,
                    *first.get_or_insert(matcher.component_evaluations),
                    "size={size}"
                );
            }
            assert!(matcher.component_evaluations > 0);
        }
    }
    #[test]
    fn repeated_resource_identity_shares_probe_work() {
        struct Counted {
            links: usize,
            stats: usize,
        }
        impl super::Probe for Counted {
            fn read_link(
                &mut self,
                path: &std::path::Path,
            ) -> std::io::Result<Option<std::path::PathBuf>> {
                self.links += 1;
                Ok((path == std::path::Path::new("/p/link")).then(|| "/public".into()))
            }
            fn stat(&mut self, _: &std::path::Path) -> std::io::Result<Option<super::Metadata>> {
                self.stats += 1;
                Ok(None)
            }
        }
        use crate::record::{Effect, Target, Via, Walk};
        let table = super::FirmlinkTable::from_text("");
        for size in [8, 16, 32] {
            let mut resolver = super::Resolver::new("/h", &table);
            let mut probe = Counted { links: 0, stats: 0 };
            let mut first = None;
            for _ in 0..size {
                let mut target =
                    Target::new("/p/link/x".into(), Effect::Read, Walk::None, Via::Operand);
                assert_eq!(
                    resolver.target(&mut target, "/p", &mut probe).unwrap(),
                    super::Identity::Public("/public/x".into())
                );
                assert_eq!(
                    (&target.path, &target.unresolved),
                    (&"/public/x".to_owned(), &"/public/x".to_owned())
                );
                let work = (probe.links, probe.stats);
                assert_eq!(work, *first.get_or_insert(work), "size={size}");
                assert_eq!(
                    resolver.home(&mut probe).unwrap(),
                    super::Identity::Public("/h".into())
                );
                assert_eq!(
                    (probe.links, probe.stats),
                    work,
                    "repeated HOME: size={size}"
                );
            }
            assert!(probe.links > 0);
            let mut root = Target::new("/h/.aws".into(), Effect::List, Walk::None, Via::Operand);
            assert!(matches!(
                resolver.target(&mut root, "/p", &mut probe).unwrap(),
                super::Identity::Public(_)
            ));
            root.effect = Effect::Read;
            assert_eq!(
                resolver.target(&mut root, "/p", &mut probe).unwrap(),
                super::Identity::Protected(super::Protection::Credential)
            );
        }
    }
    #[test]
    fn protected_ssh_candidate_is_a_policy_result_before_stat() {
        struct NoStat {
            calls: usize,
        }
        impl super::Probe for NoStat {
            fn read_link(
                &mut self,
                _: &std::path::Path,
            ) -> std::io::Result<Option<std::path::PathBuf>> {
                Ok(None)
            }
            fn stat(&mut self, _: &std::path::Path) -> std::io::Result<Option<super::Metadata>> {
                self.calls += 1;
                Err(std::io::Error::other("unexpected identity stat"))
            }
        }
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/rust-public-ssh-backups.json"
        ))
        .unwrap();
        let candidate = rows["protected_candidate"].as_str().unwrap();
        let mut probe = NoStat { calls: 0 };
        let table = super::FirmlinkTable::from_text("");
        let result = super::Resolver::new("/synthetic/home", &table)
            .ssh_denied(
                candidate,
                candidate,
                "/synthetic/project",
                "/synthetic/home",
                false,
                &mut probe,
            )
            .unwrap();
        assert_eq!(result, Some(true));
        assert_eq!(probe.calls, 0);
    }
    #[test]
    fn lexical_candidates_observe_the_evaluation_deadline() {
        let expired = std::time::Instant::now() - std::time::Duration::from_secs(1);
        assert_eq!(
            super::lexical_pattern_checked("public", "/synthetic/home", true, true, Some(expired))
                .unwrap_err()
                .kind,
            crate::CheckErrorKind::Deadline
        );
    }
    #[test]
    fn literal_api_root_does_not_expand_into_a_hidden_directory() {
        let path = format!("{}/credentials.ts", super::literal_glob_root("a/*"));
        assert_eq!(super::lexical_pattern_mode(&path, "/h", true, true), None);
        assert_eq!(
            super::lexical_pattern_mode("a/*/credentials.ts", "/h", true, true),
            Some(super::Protection::Credential)
        );
    }
}

fn near(candidate: &str, root: &str, search: bool) -> bool {
    let candidate = candidate.trim_end_matches('/').to_lowercase();
    let candidate = if candidate.is_empty() {
        "/"
    } else {
        &candidate
    };
    let root = root.to_lowercase();
    candidate == root
        || candidate.starts_with(&format!("{root}/"))
        || search && (candidate == "/" || root.starts_with(&format!("{candidate}/")))
}

fn checked_stat(
    path: &str,
    home: &str,
    resolved_home: &str,
    probe: &mut dyn Probe,
    lexical: &mut lexical::Lexical,
) -> Result<Option<Metadata>, CheckError> {
    // The caller has already decided protected spellings; keep that stop at the
    // probe boundary too, including aliases under a resolved HOME.
    if lexical
        .check_both(path, home, Some(resolved_home), false)?
        .is_some()
    {
        return Err(CheckError {
            kind: CheckErrorKind::ProbeFault,
        });
    }
    probe.stat(Path::new(path)).map_err(|_| CheckError {
        kind: CheckErrorKind::ProbeFault,
    })
}

fn same_file(
    a: &str,
    b: &str,
    home: &str,
    resolved_home: &str,
    probe: &mut dyn Probe,
    lexical: &mut lexical::Lexical,
) -> Result<bool, CheckError> {
    let x = checked_stat(a, home, resolved_home, probe, lexical)?;
    let y = checked_stat(b, home, resolved_home, probe, lexical)?;
    Ok(matches!((x, y), (Some(x), Some(y)) if (x.device, x.inode) == (y.device, y.inode)))
}

impl Resolver<'_> {
    fn ssh_denied(
        &mut self,
        target: &str,
        resolved: &str,
        cwd: &str,
        resolved_home: &str,
        search: bool,
        probe: &mut dyn Probe,
    ) -> Result<Option<bool>, CheckError> {
        let home = self.home;
        let table = self.table;
        let ssh = format!("{home}/.ssh");
        let (root, protected_root) = match resolve(
            &ssh,
            cwd,
            home,
            Some(resolved_home),
            table,
            probe,
            &mut self.lexical,
        )? {
            Resolution::Public(root) => (root, false),
            Resolution::Protected(_, root) => (root, true),
            Resolution::Bound | Resolution::InodeAlias | Resolution::InheritedInput => {
                return Ok(None);
            }
        };
        let roots = if root == ssh {
            vec![ssh.as_str()]
        } else {
            vec![ssh.as_str(), root.as_str()]
        };
        let candidates = if target == resolved {
            vec![target]
        } else {
            vec![target, resolved]
        };
        if !candidates
            .iter()
            .any(|candidate| roots.iter().any(|root| near(candidate, root, search)))
        {
            return Ok(Some(false));
        }
        if protected_root {
            return Ok(Some(true));
        }
        for candidate in candidates {
            if self
                .lexical
                .check_both(candidate, home, Some(resolved_home), true)?
                .is_some()
            {
                return Ok(Some(true));
            }
            for root in &roots {
                if same_file(
                    candidate,
                    root,
                    home,
                    resolved_home,
                    probe,
                    &mut self.lexical,
                )? {
                    return Ok(Some(true));
                }
                if search {
                    let mut parent = Path::new(root);
                    while let Some(next) = parent.parent() {
                        parent = next;
                        let Some(spelling) = parent.to_str() else {
                            return Ok(None);
                        };
                        if same_file(
                            candidate,
                            spelling,
                            home,
                            resolved_home,
                            probe,
                            &mut self.lexical,
                        )? {
                            return Ok(Some(true));
                        }
                    }
                }
                let mut parent = Path::new(candidate);
                while let Some(next) = parent.parent() {
                    parent = next;
                    let Some(spelling) = parent.to_str() else {
                        return Ok(None);
                    };
                    if !same_file(
                        spelling,
                        root,
                        home,
                        resolved_home,
                        probe,
                        &mut self.lexical,
                    )? {
                        continue;
                    }
                    let base = Path::new(candidate)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("");
                    if Some(parent) != Path::new(candidate).parent() || !ssh_public(base) {
                        return Ok(Some(true));
                    }
                    let kind = checked_stat(target, home, resolved_home, probe, &mut self.lexical)?
                        .map(|metadata| metadata.kind);
                    if kind == Some(FileKind::Directory) || search && kind != Some(FileKind::File) {
                        return Ok(Some(true));
                    }
                }
            }
        }
        Ok(Some(false))
    }
}

fn ssh_public(name: &str) -> bool {
    name == "config"
        || name.starts_with("config.")
        || name.ends_with(".pub")
        || name == "allowed_signers"
        || name.starts_with("known_hosts")
}

enum Resolution {
    Public(String),
    Protected(Protection, String),
    Bound,
    InodeAlias,
    InheritedInput,
}

fn resolve(
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
        false,
        table,
        probe,
        lexical,
    )
}

fn resolve_pattern(
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
            return match links::follow(
                prefix,
                home,
                resolved_home,
                patterned && pattern.is_none(),
                table,
                probe,
                lexical,
            )? {
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
    links::follow(
        absolute,
        home,
        resolved_home,
        patterned && pattern.is_none(),
        table,
        probe,
        lexical,
    )
}

fn rebase_pattern(raw: &str, resolved: &str, pattern: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{Effect, HostFacts, Target, Via, Walk, Word};
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Mock {
        links: BTreeMap<String, String>,
        calls: Vec<String>,
    }
    impl Probe for Mock {
        fn read_link(&mut self, path: &Path) -> io::Result<Option<PathBuf>> {
            let path = path.to_str().unwrap();
            assert!(
                lexical_literal(path, "/h").is_none(),
                "protected probe: {path}"
            );
            self.calls.push(path.into());
            Ok(self.links.get(path).map(PathBuf::from))
        }
        fn stat(&mut self, _: &Path) -> io::Result<Option<Metadata>> {
            panic!("unexpected stat");
        }
    }

    #[test]
    fn broad_patterns_compare_literal_roots_and_active_wildcards() {
        let home = "/synthetic/home-x+y@z*";
        let encoded = literal_shell_pattern(home);
        assert!(broad_root(&encoded, home, true));
        assert!(!broad_root(&format!("{encoded}*/*.md"), home, true));
        assert!(broad_root(&format!("{encoded}/**/*.md"), home, true));
    }

    #[test]
    fn item_credentials_exclude_appdata_from_the_credential_partition() {
        let table = FirmlinkTable::from_text("");
        let mut resolver = Resolver::new("/h", &table);
        assert_eq!(
            resolver.credential("/p/.env", "/h", false).unwrap(),
            Some(Protection::Environment)
        );
        assert_eq!(
            resolver
                .credential("/h/Library/Containers/x", "/h", false)
                .unwrap(),
            None
        );
    }

    #[test]
    fn credential_root_reads_remain_protected_before_public_alias_resolution() {
        let table = FirmlinkTable::from_text("");
        for (effect, via, expected) in [
            (Effect::Read, Via::Operand, true),
            (Effect::Use, Via::Operand, false),
            (Effect::Read, Via::Cwd, false),
        ] {
            let mut probe = Mock {
                links: BTreeMap::from([("/h/.docker".into(), "/public".into())]),
                calls: Vec::new(),
            };
            let mut resolver = Resolver::new("/h", &table);
            let mut target = Target::new("/h/.docker".into(), effect, Walk::None, via);
            assert_eq!(
                matches!(
                    resolver.target(&mut target, "/p", &mut probe).unwrap(),
                    Identity::Protected(Protection::Credential)
                ),
                expected
            );
            if expected {
                assert!(probe.calls.is_empty());
            }
        }
    }

    #[test]
    fn aliased_credential_root_reads_keep_the_post_resolution_role() {
        let table = FirmlinkTable::from_text("");
        for (effect, via, expected) in [
            (Effect::Read, Via::Operand, true),
            (Effect::Use, Via::Operand, false),
            (Effect::Read, Via::Cwd, false),
        ] {
            let mut probe = Mock {
                links: BTreeMap::from([("/p/link".into(), "/h/.docker".into())]),
                calls: Vec::new(),
            };
            let mut resolver = Resolver::new("/h", &table);
            let mut target = Target::new("/p/link".into(), effect, Walk::None, via);
            assert_eq!(
                matches!(
                    resolver.target(&mut target, "/p", &mut probe).unwrap(),
                    Identity::Protected(Protection::Credential)
                ),
                expected
            );
            assert!(probe.calls.contains(&"/p/link".to_owned()));
        }
    }

    #[test]
    fn lexical_api_keeps_relative_and_short_pattern_boundaries() {
        assert_eq!(
            lexical_pattern("/h/Library/Containers/x", "/h", false),
            Some(Protection::AppData)
        );
        assert_eq!(
            lexical_pattern("/b*", "/h", true),
            Some(Protection::Credential)
        );
        assert_eq!(lexical_pattern("/b*.txt", "/h", true), None);
        assert_eq!(
            lexical_pattern(".aws/c*", "/h", true),
            Some(Protection::Credential)
        );
        assert_eq!(
            lexical_pattern("/.config/gh/host*", "/h", true),
            Some(Protection::Credential)
        );
    }

    #[test]
    fn catalog_controls_physical_parent_transition() {
        let packet: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/rust-m2-filesystem.json"))
                .unwrap();
        let rows: Vec<_> = packet["paths"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r.get("catalog").is_some() && r["operation"] != "shell")
            .collect();
        assert!(!rows.is_empty(), "missing catalog non-shell partition");
        for row in rows {
            let table = FirmlinkTable::from_text(row["catalog"].as_str().unwrap());
            let mut resolver = Resolver::new("/h", &table);
            let mut probe = Mock {
                links: row
                    .get("links")
                    .map(|v| serde_json::from_value(v.clone()).unwrap())
                    .unwrap_or_default(),
                calls: Vec::new(),
            };
            let mut target = Target::new(
                row["path"].as_str().unwrap().into(),
                Effect::Use,
                Walk::None,
                Via::Operand,
            );
            let result = resolver
                .target(
                    &mut target,
                    row["cwd"].as_str().unwrap_or("/project"),
                    &mut probe,
                )
                .unwrap();
            if let Some(kind) = row["protected"].as_str() {
                assert!(
                    matches!(result, Identity::Protected(p) if format!("{p:?}") == kind),
                    "{}: {result:?}",
                    row["id"]
                );
            } else {
                assert_eq!(
                    result,
                    Identity::Public(row["public"].as_str().unwrap().into()),
                    "{}",
                    row["id"]
                );
            }
        }
    }

    #[test]
    fn target_preserves_raw_input_and_updates_both_linked_fields() {
        let mut word = Word::literal("../item".into());
        word.expands = true;
        let mut target = Target::from_word(
            &word,
            "/System/Volumes/Data/a/link/./tail",
            HostFacts {
                home: "/h",
                user: None,
            },
            Effect::Use,
            Walk::Visible,
        );
        assert_eq!(target.path, "/a/link/item");
        assert_eq!(
            target.unresolved,
            "/System/Volumes/Data/a/link/./tail/../item"
        );
        target.command = Some(3);
        target.sends = true;
        target.search = true;
        let table = FirmlinkTable::from_text("");
        let mut resolver = Resolver::new("/h", &table);
        let mut probe = Mock {
            links: BTreeMap::from([("/System/Volumes/Data/a/link".into(), "/public".into())]),
            calls: Vec::new(),
        };
        assert_eq!(
            resolver
                .target(&mut target, "/project", &mut probe)
                .unwrap(),
            Identity::Public("/public/item".into())
        );
        assert_eq!(target.path, "/public/item");
        assert_eq!(target.unresolved, target.path);
        assert!(target.expands && target.sends && target.search);
        assert_eq!(
            (target.effect, target.walk, target.via, target.command),
            (Effect::Use, Walk::Visible, Via::Operand, Some(3))
        );
    }

    #[test]
    fn literal_name_and_tool_glob_skip_identity() {
        let table = FirmlinkTable::from_text("");
        let mut resolver = Resolver::new("/h", &table);
        let mut probe = Mock::default();
        for (path, effect, via, glob) in [
            ("/public/link/*", Effect::Name, Via::Operand, false),
            ("/public/link/*.txt", Effect::Read, Via::Tool, true),
        ] {
            let mut target = Target::new(path.into(), effect, Walk::None, via);
            target.glob = glob;
            assert_eq!(
                resolver
                    .target(&mut target, "/project", &mut probe)
                    .unwrap(),
                Identity::Public(path.into())
            );
        }
        assert!(probe.calls.is_empty());
    }

    #[test]
    fn evaluation_keeps_no_follow_input() {
        let table = FirmlinkTable::from_text("");
        let mut resolver = Resolver::new("/h", &table);
        let mut probe = Mock::default();
        for path in ["/a/./item", "/b/../item"] {
            let mut target = Target::new(path.into(), Effect::Use, Walk::None, Via::Operand);
            let unresolved = target.unresolved.clone();
            let clean = target.path.clone();
            let identity = resolver
                .target(&mut target, "/project", &mut probe)
                .unwrap();
            assert_eq!(identity, Identity::Public(clean.clone()));
            assert_eq!(target.path, clean);
            assert_eq!(target.unresolved, unresolved);
        }
        assert!(probe.calls.iter().any(|p| p == "/h"));
        assert!(probe.calls.iter().all(|p| !p.contains("/./")));
        let mut target = Target::new("//*.txt".into(), Effect::Use, Walk::None, Via::Operand);
        target.glob = true;
        assert_eq!(
            resolver
                .target(&mut target, "/project", &mut probe)
                .unwrap(),
            Identity::Public("/*.txt".into())
        );
        // policy::Inspection::target reads unresolved to distinguish relative tilde spellings.
        assert_eq!(target.unresolved, "//*.txt");
    }

    #[test]
    fn expands_without_glob_follows_only_public_prefix() {
        let packet: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/rust-m2-filesystem.json"))
                .unwrap();
        let row = packet["paths"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "expands-only-suffix")
            .unwrap();
        let mut word = Word::literal(row["path"].as_str().unwrap().into());
        word.expands = true;
        let mut target = Target::from_word(
            &word,
            "/project",
            HostFacts {
                home: "/h",
                user: None,
            },
            Effect::Use,
            Walk::None,
        );
        let table = FirmlinkTable::from_text("");
        let mut resolver = Resolver::new("/h", &table);
        let mut probe = Mock {
            links: serde_json::from_value(row["links"].clone()).unwrap(),
            calls: Vec::new(),
        };
        assert_eq!(
            resolver
                .target(&mut target, "/project", &mut probe)
                .unwrap(),
            Identity::Public(row["public"].as_str().unwrap().into())
        );
        assert_eq!(
            probe.calls.last().unwrap(),
            row["last_probe"].as_str().unwrap()
        );
        assert!(probe.calls.iter().all(|path| !path.contains("${suffix}")));
    }
}
