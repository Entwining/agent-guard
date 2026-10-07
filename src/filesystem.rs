//! Lexical checks precede identity probes. Stat is confined to SSH identity.

mod glob;
mod links;

pub use links::FirmlinkTable;

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

pub struct DiskProbe;

impl Probe for DiskProbe {
    fn stat(&mut self, path: &Path) -> io::Result<Option<Metadata>> {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(path) {
            Ok(info) => Ok(Some(Metadata {
                device: info.dev(),
                inode: info.ino(),
                kind: if info.is_dir() {
                    FileKind::Directory
                } else if info.is_file() {
                    FileKind::File
                } else {
                    FileKind::Other
                },
            })),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
    fn read_link(&mut self, path: &Path) -> io::Result<Option<PathBuf>> {
        match std::fs::read_link(path) {
            Ok(target) => Ok(Some(target)),
            // Darwin ENAMETOOLONG shares Go's benign non-link contract.
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::InvalidInput
                        | io::ErrorKind::NotADirectory
                ) || error.raw_os_error() == Some(63) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
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
    unfirmlink(clean.to_str().unwrap_or(""))
}

pub(crate) fn absolute_input(path: &str, cwd: &str, home: &str) -> String {
    let expanded = if let Some(tail) = path.strip_prefix("~/") {
        format!("{home}/{tail}")
    } else if path == "~" {
        home.to_owned()
    } else {
        path.to_owned()
    };
    let expanded = strip_file_url(&expanded);
    if expanded.starts_with('/') {
        expanded.to_owned()
    } else {
        format!("{cwd}/{expanded}")
    }
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
    let prefix = "/system/volumes/data";
    if path.eq_ignore_ascii_case(prefix) {
        "/".to_owned()
    } else if path.to_ascii_lowercase().starts_with(prefix)
        && path.as_bytes().get(prefix.len()) == Some(&b'/')
    {
        path[prefix.len()..].to_owned()
    } else {
        path.to_owned()
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

/// Go's public App Data reason is narrower than the trial's conservative
/// protection predicate. This selects prose only and never permits a target.
pub(crate) fn appdata_reason(path: &str, home: &str, patterned: bool) -> bool {
    // native/filesystem/appdata.go:11-54 and glob.go:65-77.
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
    // D22 retains conservative group reach; P1 quoting gates brace/glob expansion.
    let mut matcher = glob::Matcher::default();
    for candidate in glob::alternatives(path, patterned) {
        crate::check_deadline(deadline)?;
        if let Some(kind) = lexical_candidate(
            &candidate,
            home,
            patterned || candidate != path,
            hidden,
            &mut matcher,
        ) {
            return Ok(Some(kind));
        }
    }
    Ok(None)
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
    let path = path.to_lowercase();
    let home = home.to_lowercase();
    let literal = path == "/"
        || path == home
        || path == format!("{home}/library")
        || home.starts_with(&format!("{}/", path.trim_end_matches('/')));
    if literal || !patterned {
        return Ok(literal);
    }
    let mut candidates = vec![home.clone(), format!("{home}/library")];
    for tree in [
        "containers",
        "group containers",
        "mobile documents",
        "cloudstorage",
    ] {
        candidates.push(format!("{home}/library/{tree}"));
        candidates.push(format!("{home}/library/{tree}/x"));
    }
    // Go's recursive-glob owner checks both protected witnesses and the
    // lexical prefix (native/filesystem/appdata.go:77-91).
    let mut matcher = glob::Matcher::default();
    for pattern in glob::alternatives(&path, true) {
        crate::check_deadline(deadline)?;
        let prefix = pattern
            .find(['*', '?', '['])
            .map_or(pattern.as_str(), |at| &pattern[..at])
            .trim_end_matches('/');
        for candidate in &candidates {
            if matcher.path_checked(&pattern, candidate, deadline)? {
                return Ok(true);
            }
        }
        if pattern.contains("**")
            && (prefix == home
                || prefix == format!("{home}/library")
                || home.starts_with(&format!("{prefix}/")))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn lexical_candidate(
    path: &str,
    home: &str,
    patterned: bool,
    hidden: bool,
    matcher: &mut glob::Matcher,
) -> Option<Protection> {
    let spelling = path;
    let path = path.to_lowercase();
    let patterned = patterned && path.contains(['*', '?', '[', '{', '(']);
    let library = format!("{home}/Library").to_lowercase();
    for owner in [
        "containers",
        "group containers",
        "mobile documents",
        "cloudstorage",
    ] {
        let root = format!("{library}/{owner}");
        let p: Vec<_> = path.split('/').collect();
        let r: Vec<_> = root.split('/').collect();
        if path == root
            || path.starts_with(&format!("{root}/"))
            || patterned
                && (matcher.path(&path, &root)
                    || p.len() > r.len()
                        && r.iter().enumerate().all(|(index, part)| {
                            p[index] == "**" || matcher.component(p[index], part)
                        }))
        {
            return Some(Protection::AppData);
        }
    }
    let parts: Vec<&str> = path.split('/').collect();
    if let Some(index) = parts.iter().position(|part| {
        *part == ".ssh" || patterned && part.starts_with('.') && matcher.component(part, ".ssh")
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
    if parts.iter().any(|part| {
        *part == ".env"
            || part.starts_with(".env.")
            || patterned
                && !part.trim_matches(['*', '?']).is_empty()
                && (matcher.component(part, ".env") || matcher.component(part, ".env.x"))
    }) {
        return Some(Protection::Environment);
    }
    // Go separates sensitive files from read-only credential roots
    // (native/filesystem/credentials.go:8-10, 135-142). A public child may
    // traverse a root's metadata; reading the root is checked by the resolver.
    if parts.contains(&"private-keys-v1.d")
        || base.starts_with("credentials")
            && path
                .rsplit_once('/')
                .is_some_and(|(parent, _)| parent.ends_with("/.aws"))
        || [".npmrc", ".netrc", ".git-credentials", ".pypirc", ".pgpass"].contains(&base)
        || base.starts_with(".zprofile")
        || base.starts_with(".zsh_history")
        || base.starts_with("auth.json")
        || base.starts_with(".credentials.json")
        || base.ends_with(".pem")
        || base.ends_with(".key")
        || [
            "/.docker/config.json",
            "/.kube/config",
            "/.config/gh/hosts.yml",
        ]
        .iter()
        .any(|suffix| path.ends_with(suffix))
        || path.contains("/.cargo/credentials")
    {
        return Some(Protection::Credential);
    }
    if !patterned {
        return None;
    }
    if base.trim_matches(['*', '?']).is_empty() {
        let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
        if [".ssh", ".aws", ".gnupg"].contains(&parent.rsplit('/').next().unwrap_or(""))
            || listed_directories().any(|dir| parent.ends_with(&format!("/{dir}")))
        {
            return Some(Protection::Credential);
        }
    }
    for listed in SENSITIVE {
        if matcher.path(listed, &path) {
            return Some(Protection::Credential);
        }
        let tail: Vec<_> = listed.trim_start_matches("**/").split('/').collect();
        if tail.len() > parts.len()
            || tail[tail.len() - 1].trim_matches('*').is_empty()
            || base.trim_matches(['*', '?']).is_empty()
        {
            continue;
        }
        let offset = parts.len() - tail.len();
        if tail[..tail.len() - 1]
            .iter()
            .enumerate()
            .all(|(index, part)| {
                matcher.visible_component(parts[offset + index], &part.replace('*', "x"), hidden)
            })
            && if tail.len() > 1 {
                glob::visible_intersects(base, tail[tail.len() - 1], hidden)
            } else {
                matcher.visible_component(base, &tail[0].replace('*', "x"), hidden)
            }
        {
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
    SENSITIVE.iter().filter_map(|path| {
        path.trim_start_matches("**/")
            .rsplit_once('/')
            .map(|(dir, _)| dir)
            .filter(|dir| !dir.contains('*'))
    })
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

pub(crate) fn credential_read(path: &str, home: &str, patterned: bool) -> Option<Protection> {
    lexical_pattern(path, home, patterned)
        .filter(|kind| *kind != Protection::AppData)
        .or_else(|| (!patterned && sensitive_root(path, home)).then_some(Protection::Credential))
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
    targets: std::collections::BTreeMap<TargetIdentity, (Identity, String, String)>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct TargetIdentity {
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
        Self {
            home,
            table,
            deadline: None,
            resolved_home: None,
            targets: std::collections::BTreeMap::new(),
        }
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
            targets: std::collections::BTreeMap::new(),
        }
    }

    pub(crate) fn home(&mut self, probe: &mut dyn Probe) -> Result<Identity, CheckError> {
        crate::check_deadline(self.deadline)?;
        if let Some(home) = &self.resolved_home {
            return Ok(home.clone());
        }
        let home = match resolve(self.home, self.home, self.home, None, self.table, probe)? {
            Resolution::Public(path) => Identity::Public(normalize(&path, self.home, self.home)),
            Resolution::Protected(kind, _) => Identity::Protected(kind),
            Resolution::Bound => Identity::Bound,
        };
        self.resolved_home = Some(home.clone());
        Ok(home)
    }

    pub(crate) fn target(
        &mut self,
        target: &mut crate::record::Target,
        cwd: &str,
        probe: &mut dyn Probe,
    ) -> Result<Identity, CheckError> {
        crate::check_deadline(self.deadline)?;
        let key = TargetIdentity {
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
        if let Some((identity, path, unresolved)) = self.targets.get(&key) {
            target.path = path.clone();
            target.unresolved = unresolved.clone();
            return Ok(identity.clone());
        }
        let identity = self.resolve_target(target, cwd, probe)?;
        self.targets.insert(
            key,
            (
                identity.clone(),
                target.path.clone(),
                target.unresolved.clone(),
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
        if path.ends_with("/.ssh") {
            return Ok(Identity::Protected(Protection::SshPrivate));
        }
        if let Some(kind) = lexical_pattern_checked(
            &path,
            self.home,
            target.glob,
            target.glob_hidden,
            self.deadline,
        )? {
            return Ok(Identity::Protected(kind));
        }
        if target.effect == Effect::Read
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
            cwd,
            self.home,
            Some(&resolved_home),
            target.glob || target.expands,
            self.table,
            probe,
        )? {
            Resolution::Public(resolved) => {
                if resolved != raw {
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
        };
        if let Some(kind) = lexical_pattern_checked(
            &resolved,
            self.home,
            target.glob,
            target.glob_hidden,
            self.deadline,
        )?
        .or(lexical_pattern_checked(
            &resolved,
            &resolved_home,
            target.glob,
            target.glob_hidden,
            self.deadline,
        )?) {
            return Ok(Identity::Protected(kind));
        }
        if target.effect == Effect::Read
            && !target.glob
            && (target.via != Via::Cwd || target.search)
            && sensitive_root(&resolved, &resolved_home)
        {
            return Ok(Identity::Protected(Protection::Credential));
        }
        // The broad-root owner wins before subordinate SSH metadata comparisons.
        let search = target.walk != Walk::None;
        if (search || target.glob)
            && broad_root_checked(&resolved, &resolved_home, target.glob, self.deadline)?
        {
            return Ok(Identity::Public(resolved));
        }
        if matches!(target.effect, Effect::Read | Effect::Write | Effect::List) {
            match ssh_denied(
                &path,
                &resolved,
                cwd,
                self.home,
                &resolved_home,
                target.search,
                self.table,
                probe,
            )? {
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
    fn repeated_pattern_components_share_match_work() {
        for size in [8, 16, 32] {
            let mut matcher = super::glob::Matcher::default();
            let mut first = None;
            for _ in 0..size {
                assert_eq!(
                    super::lexical_candidate(
                        "/h/project/public[a-z]/nested/public.json",
                        "/h",
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
        let result = super::ssh_denied(
            candidate,
            candidate,
            "/synthetic/project",
            "/synthetic/home",
            "/synthetic/home",
            false,
            &table,
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
) -> Result<Option<Metadata>, CheckError> {
    // The caller has already decided protected spellings; keep that stop at the
    // probe boundary too, including aliases under a resolved HOME.
    if lexical(path, home)
        .or_else(|| lexical(path, resolved_home))
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
) -> Result<bool, CheckError> {
    let x = checked_stat(a, home, resolved_home, probe)?;
    let y = checked_stat(b, home, resolved_home, probe)?;
    Ok(matches!((x, y), (Some(x), Some(y)) if (x.device, x.inode) == (y.device, y.inode)))
}

#[allow(clippy::too_many_arguments)]
fn ssh_denied(
    target: &str,
    resolved: &str,
    cwd: &str,
    home: &str,
    resolved_home: &str,
    search: bool,
    table: &FirmlinkTable,
    probe: &mut dyn Probe,
) -> Result<Option<bool>, CheckError> {
    let ssh = format!("{home}/.ssh");
    let (root, protected_root) = match resolve(&ssh, cwd, home, Some(resolved_home), table, probe)?
    {
        Resolution::Public(root) => (root, false),
        Resolution::Protected(_, root) => (root, true),
        Resolution::Bound => return Ok(None),
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
        if lexical(candidate, home)
            .or_else(|| lexical(candidate, resolved_home))
            .is_some()
        {
            return Ok(Some(true));
        }
        for root in &roots {
            if same_file(candidate, root, home, resolved_home, probe)? {
                return Ok(Some(true));
            }
            if search {
                let mut parent = Path::new(root);
                while let Some(next) = parent.parent() {
                    parent = next;
                    let Some(spelling) = parent.to_str() else {
                        return Ok(None);
                    };
                    if same_file(candidate, spelling, home, resolved_home, probe)? {
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
                if !same_file(spelling, root, home, resolved_home, probe)? {
                    continue;
                }
                let base = Path::new(candidate)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                if Some(parent) != Path::new(candidate).parent() || !ssh_public(base) {
                    return Ok(Some(true));
                }
                let kind =
                    checked_stat(target, home, resolved_home, probe)?.map(|metadata| metadata.kind);
                if kind == Some(FileKind::Directory) || search && kind != Some(FileKind::File) {
                    return Ok(Some(true));
                }
            }
        }
    }
    Ok(Some(false))
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
}

fn resolve(
    path: &str,
    cwd: &str,
    home: &str,
    resolved_home: Option<&str>,
    table: &FirmlinkTable,
    probe: &mut dyn Probe,
) -> Result<Resolution, CheckError> {
    links::follow(
        &absolute_input(path, cwd, home),
        home,
        resolved_home,
        false,
        table,
        probe,
    )
}

fn resolve_pattern(
    path: &str,
    cwd: &str,
    home: &str,
    resolved_home: Option<&str>,
    patterned: bool,
    table: &FirmlinkTable,
    probe: &mut dyn Probe,
) -> Result<Resolution, CheckError> {
    let absolute = absolute_input(path, cwd, home);
    if patterned {
        let segments: Vec<_> = absolute.split('/').collect();
        if let Some(index) = segments
            .iter()
            .position(|part| part.contains(['*', '?', '[', '{', '$', '`']))
        {
            let prefix = segments[..index].join("/");
            let prefix = if prefix.is_empty() { "/" } else { &prefix };
            return match links::follow(prefix, home, resolved_home, patterned, table, probe)? {
                Resolution::Public(resolved) if resolved == prefix => {
                    Ok(Resolution::Public(absolute))
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
    links::follow(&absolute, home, resolved_home, patterned, table, probe)
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
    fn item_credentials_exclude_appdata_from_the_credential_partition() {
        assert_eq!(
            credential_read("/p/.env", "/h", false),
            Some(Protection::Environment)
        );
        assert_eq!(
            credential_read("/h/Library/Containers/x", "/h", false),
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
        assert_eq!(lexical_pattern("/b*", "/h", true), None);
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
        for (effect, via, glob) in [
            (Effect::Name, Via::Operand, false),
            (Effect::Read, Via::Tool, true),
        ] {
            let mut target = Target::new("/public/link/*".into(), effect, Walk::None, via);
            target.glob = glob;
            assert_eq!(
                resolver
                    .target(&mut target, "/project", &mut probe)
                    .unwrap(),
                Identity::Public("/public/link/*".into())
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
