//! Lexical checks precede identity and public search file-kind probes.

mod glob;
mod lexical;
mod links;
mod paths;
#[expect(
    clippy::disallowed_methods,
    reason = "Raw filesystem identity calls are confined to this probe; callers own lexical preflight."
)]
mod probe;
mod protection;
mod resolution;
mod resolver;
mod ssh;

pub(crate) use glob::{
    component as parameter_pattern_matches, escape_literal as literal_glob_root, grep_pattern,
    shell_pattern,
};
pub use links::FirmlinkTable;
pub use probe::DiskProbe;
pub(crate) use probe::canonicalize_home;

use crate::{CheckError, CheckErrorKind};
use std::{
    io,
    path::{Path, PathBuf},
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

pub(crate) use paths::{
    absolute_input, absolute_pattern, descriptor_path, literal_shell_pattern, strip_file_url,
    strip_path_aliases,
};
pub use paths::{expand_home, normalize};
use paths::{unfirmlink, unfirmlink_path};
pub(crate) use protection::appdata_reason;
use protection::{SENSITIVE, broad_prepared, lexical_candidate, sensitive_root};
pub use protection::{appdata_fragment, broad_root, lexical, lexical_literal};
#[cfg(test)]
use protection::{lexical_pattern, lexical_pattern_checked, lexical_pattern_mode};
use resolution::{Resolution, rebase_pattern, resolve, resolve_pattern};
pub(crate) use resolver::Resolver;
use ssh::{checked_stat, near, same_file, ssh_public};
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

#[cfg(test)]
mod tests;
