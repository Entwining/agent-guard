//! Lexical checks precede every readlink-only identity probe.

mod glob;

use crate::{CheckError, CheckErrorKind};
use std::{
    io,
    path::{Component, Path, PathBuf},
};

pub trait Probe {
    fn read_link(&mut self, path: &Path) -> io::Result<Option<PathBuf>>;
}

pub struct DiskProbe;

impl Probe for DiskProbe {
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
    let expanded = if let Some(tail) = path.strip_prefix("~/") {
        format!("{home}/{tail}")
    } else if path == "~" {
        home.to_owned()
    } else {
        path.to_owned()
    };
    let joined = if expanded.starts_with('/') {
        PathBuf::from(expanded)
    } else {
        Path::new(cwd).join(expanded)
    };
    let mut clean = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::ParentDir => {
                clean.pop();
            }
            Component::CurDir => {}
            part => clean.push(part.as_os_str()),
        }
    }
    // Inputs are UTF-8; components preserve those bytes without lossy OS conversion.
    let clean = clean.to_str().unwrap_or("");
    clean
        .strip_prefix("/System/Volumes/Data")
        .filter(|tail| tail.starts_with('/'))
        .unwrap_or(&clean)
        .to_owned()
}

pub fn lexical(path: &str, home: &str) -> Option<Protection> {
    for candidate in glob::alternatives(path) {
        if let Some(kind) = lexical_candidate(&candidate, home) {
            return Some(kind);
        }
    }
    None
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
        if !tail.is_empty()
            && (tail.len() != 1
                || !(tail[0] == "config"
                    || tail[0].starts_with("config.")
                    || tail[0].ends_with(".pub")
                    || tail[0] == "allowed_signers"
                    || tail[0].starts_with("known_hosts")))
        {
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
    if parts.iter().any(|p| *p == "private-keys-v1.d")
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
            && glob::intersects(base, tail[tail.len() - 1])
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
    let path = normalize(path, cwd, home);
    if path.ends_with("/.ssh") {
        return Ok(Identity::Protected(Protection::SshPrivate));
    }
    if let Some(kind) = lexical(&path, home) {
        return Ok(Identity::Protected(kind));
    }
    let resolved_home = match resolve(home, home, home, None, probe)? {
        Identity::Public(path) => path,
        Identity::Bound => return Ok(Identity::Bound),
        Identity::Protected(kind) => return Ok(Identity::Protected(kind)),
    };
    resolve(&path, cwd, home, Some(&resolved_home), probe)
}

fn resolve(
    path: &str,
    cwd: &str,
    home: &str,
    resolved_home: Option<&str>,
    probe: &mut dyn Probe,
) -> Result<Identity, CheckError> {
    let mut current = normalize(path, cwd, home);
    for _ in 0..40 {
        if let Some(kind) = lexical(&current, home)
            .or_else(|| resolved_home.and_then(|home| lexical(&current, home)))
        {
            return Ok(Identity::Protected(kind));
        }
        let mut prefix = PathBuf::new();
        let parts: Vec<_> = Path::new(&current).components().collect();
        let mut resolved = None;
        for (index, part) in parts.iter().enumerate() {
            prefix.push(part.as_os_str());
            let Some(spelling) = prefix.to_str() else {
                return Ok(Identity::Bound);
            };
            if let Some(kind) = lexical(spelling, home)
                .or_else(|| resolved_home.and_then(|home| lexical(spelling, home)))
            {
                return Ok(Identity::Protected(kind));
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
                    return Ok(Identity::Bound);
                };
                resolved = Some(normalize(joined, cwd, home));
                break;
            }
        }
        match resolved {
            Some(next) => current = next,
            None => return Ok(Identity::Public(current)),
        }
    }
    Ok(Identity::Bound)
}
