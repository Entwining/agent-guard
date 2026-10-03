//! Lexical checks precede every readlink-only identity probe.

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
    let clean = clean.to_string_lossy();
    clean
        .strip_prefix("/System/Volumes/Data")
        .filter(|tail| tail.starts_with('/'))
        .unwrap_or(&clean)
        .to_owned()
}

pub fn lexical(path: &str, home: &str) -> Option<Protection> {
    let path = path.to_lowercase();
    let library = format!("{home}/Library").to_lowercase();
    for owner in [
        "containers",
        "group containers",
        "mobile documents",
        "cloudstorage",
    ] {
        let root = format!("{library}/{owner}");
        if path == root || path.starts_with(&format!("{root}/")) {
            return Some(Protection::AppData);
        }
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts.contains(&".ssh") {
        return Some(Protection::SshPrivate);
    }
    let base = parts.last().copied().unwrap_or("");
    if base == ".env.example" || base == ".env.age" {
        return None;
    }
    if parts
        .iter()
        .any(|part| *part == ".env" || part.starts_with(".env.") || part.starts_with(".env*"))
    {
        return Some(Protection::Environment);
    }
    if parts
        .iter()
        .any(|p| matches!(*p, ".aws" | ".gnupg" | "private-keys-v1.d"))
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
    None
}

pub fn identify(
    path: &str,
    cwd: &str,
    home: &str,
    probe: &mut dyn Probe,
) -> Result<Identity, CheckError> {
    let mut current = normalize(path, cwd, home);
    for _ in 0..40 {
        if let Some(kind) = lexical(&current, home) {
            return Ok(Identity::Protected(kind));
        }
        let mut prefix = PathBuf::new();
        let parts: Vec<_> = Path::new(&current).components().collect();
        let mut resolved = None;
        for (index, part) in parts.iter().enumerate() {
            prefix.push(part.as_os_str());
            let spelling = prefix.to_string_lossy();
            if let Some(kind) = lexical(&spelling, home) {
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
                resolved = Some(normalize(&joined.to_string_lossy(), cwd, home));
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
