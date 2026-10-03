//! Lexical checks precede identity probes. Stat is confined to SSH identity.

mod glob;

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
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::InvalidInput
                        | io::ErrorKind::NotADirectory
                ) =>
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

#[derive(Debug, PartialEq, Eq)]
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

fn absolute_input(path: &str, cwd: &str, home: &str) -> String {
    let expanded = if let Some(tail) = path.strip_prefix("~/") {
        format!("{home}/{tail}")
    } else if path == "~" {
        home.to_owned()
    } else {
        path.to_owned()
    };
    let joined = if expanded.starts_with('/') {
        expanded
    } else {
        format!("{cwd}/{expanded}")
    };
    unfirmlink(&joined)
}

fn unfirmlink(path: &str) -> String {
    let prefix = "/system/volumes/data";
    if path.to_ascii_lowercase().starts_with(prefix)
        && path.as_bytes().get(prefix.len()) == Some(&b'/')
    {
        path[prefix.len()..].to_owned()
    } else {
        path.to_owned()
    }
}

pub fn lexical(path: &str, home: &str) -> Option<Protection> {
    for candidate in glob::alternatives(path) {
        if let Some(kind) = lexical_candidate(&candidate, home) {
            return Some(kind);
        }
    }
    None
}

pub fn broad_root(path: &str, home: &str) -> bool {
    let path = path.to_lowercase();
    let home = home.to_lowercase();
    path == "/"
        || path == home
        || path == format!("{home}/library")
        || home.starts_with(&format!("{}/", path.trim_end_matches('/')))
        || path.contains(['*', '?', '[', '{', '('])
            && [home.clone(), format!("{home}/library")]
                .iter()
                .any(|candidate| {
                    glob::alternatives(&path)
                        .iter()
                        .any(|pattern| glob::path(pattern, candidate))
                })
}

fn lexical_candidate(path: &str, home: &str) -> Option<Protection> {
    let path = path.to_lowercase();
    let patterned = path.contains(['*', '?', '[', '{', '(']);
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
                && (glob::path(&path, &root)
                    || p.len() > r.len()
                        && r.iter().enumerate().all(|(index, part)| {
                            p[index] == "**" || glob::component(p[index], part)
                        }))
        {
            return Some(Protection::AppData);
        }
    }
    let parts: Vec<&str> = path.split('/').collect();
    if let Some(index) = parts.iter().position(|part| {
        *part == ".ssh" || patterned && part.starts_with('.') && glob::component(part, ".ssh")
    }) {
        let tail = &parts[index + 1..];
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
                && (glob::component(part, ".env") || glob::component(part, ".env.x"))
    }) {
        return Some(Protection::Environment);
    }
    if parts.contains(&"private-keys-v1.d")
        || [".aws", ".gnupg"].contains(&base)
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
    for listed in SENSITIVE {
        if glob::path(listed, &path) {
            return Some(Protection::Credential);
        }
        let tail: Vec<_> = listed.trim_start_matches("**/").split('/').collect();
        if tail.len() > parts.len() || base.trim_matches(['*', '?']).is_empty() {
            continue;
        }
        let offset = parts.len() - tail.len();
        if tail[..tail.len() - 1]
            .iter()
            .enumerate()
            .all(|(index, part)| glob::component(parts[offset + index], &part.replace('*', "x")))
            && if tail.len() > 1 {
                glob::intersects(base, tail[tail.len() - 1])
            } else {
                glob::component(base, &tail[0].replace('*', "x"))
            }
        {
            return Some(Protection::Credential);
        }
    }
    None
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
    let raw_path = absolute_input(path, cwd, home);
    let path = normalize(path, cwd, home);
    if path.ends_with("/.ssh") {
        return Ok(Identity::Protected(Protection::SshPrivate));
    }
    if let Some(kind) = lexical(&path, home) {
        return Ok(Identity::Protected(kind));
    }
    let resolved_home = match resolve(home, home, home, None, false, probe)? {
        Resolution::Public(path) => path,
        Resolution::Bound => return Ok(Identity::Bound),
        Resolution::Protected(kind, _) => return Ok(Identity::Protected(kind)),
    };
    let resolved = match resolve(&raw_path, cwd, home, Some(&resolved_home), false, probe)? {
        Resolution::Public(path) => path,
        Resolution::Protected(kind, _) => return Ok(Identity::Protected(kind)),
        Resolution::Bound => return Ok(Identity::Bound),
    };
    // Broad traversal is already denied by its owner; do not replace its HOME
    // scope with a subordinate SSH reason or perform unnecessary stat probes.
    if search && broad_root(&resolved, &resolved_home) {
        return Ok(Identity::Public(resolved));
    }
    match ssh_denied(&path, &resolved, cwd, home, &resolved_home, search, probe)? {
        Some(true) => return Ok(Identity::Protected(Protection::SshPrivate)),
        None => return Ok(Identity::Bound),
        Some(false) => {}
    }
    Ok(Identity::Public(resolved))
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
    if a == b {
        return Ok(true);
    }
    let x = checked_stat(a, home, resolved_home, probe)?;
    let y = checked_stat(b, home, resolved_home, probe)?;
    Ok(matches!((x, y), (Some(x), Some(y)) if (x.device, x.inode) == (y.device, y.inode)))
}

fn ssh_denied(
    target: &str,
    resolved: &str,
    cwd: &str,
    home: &str,
    resolved_home: &str,
    search: bool,
    probe: &mut dyn Probe,
) -> Result<Option<bool>, CheckError> {
    let ssh = format!("{home}/.ssh");
    let (root, protected_root) = match resolve(&ssh, cwd, home, Some(resolved_home), true, probe)? {
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
    allow_ssh_root: bool,
    probe: &mut dyn Probe,
) -> Result<Resolution, CheckError> {
    let mut current = absolute_input(path, cwd, home);
    for _ in 0..40 {
        if !allow_ssh_root && normalize(&current, cwd, home).ends_with("/.ssh") {
            return Ok(Resolution::Protected(
                Protection::SshPrivate,
                normalize(&current, cwd, home),
            ));
        }
        if let Some(kind) = lexical(&current, home)
            .or_else(|| resolved_home.and_then(|home| lexical(&current, home)))
        {
            return Ok(Resolution::Protected(kind, normalize(&current, cwd, home)));
        }
        let mut prefix = PathBuf::new();
        let parts: Vec<_> = Path::new(&current).components().collect();
        let mut resolved = None;
        for (index, part) in parts.iter().enumerate() {
            prefix.push(part.as_os_str());
            let Some(spelling) = prefix.to_str() else {
                return Ok(Resolution::Bound);
            };
            if let Some(kind) = lexical(spelling, home)
                .or_else(|| resolved_home.and_then(|home| lexical(spelling, home)))
                .or_else(|| lexical(&normalize(spelling, cwd, home), home))
                .or_else(|| {
                    resolved_home.and_then(|home| lexical(&normalize(spelling, cwd, home), home))
                })
            {
                return Ok(Resolution::Protected(kind, normalize(&current, cwd, home)));
            }
            if spelling.contains(['*', '?', '[', '(']) {
                break;
            }
            let target = probe.read_link(&prefix).map_err(|_| CheckError {
                kind: CheckErrorKind::ProbeFault,
            })?;
            if let Some(target) = target {
                let base = prefix.parent().unwrap_or(Path::new("/"));
                let mut joined = if target.is_absolute() {
                    target
                } else {
                    base.join(target)
                };
                for remaining in &parts[index + 1..] {
                    joined.push(remaining.as_os_str());
                }
                let Some(joined) = joined.to_str() else {
                    return Ok(Resolution::Bound);
                };
                resolved = Some(absolute_input(joined, cwd, home));
                break;
            }
        }
        match resolved {
            Some(next) => current = next,
            None => return Ok(Resolution::Public(normalize(&current, cwd, home))),
        }
    }
    Ok(Resolution::Bound)
}
