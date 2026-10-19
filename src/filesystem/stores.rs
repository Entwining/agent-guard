//! Where clients keep credentials. `SENSITIVE` recognizes credential names
//! wherever they appear; a store is protected by location, so its files stay
//! protected where a variable or link moves it, and reading a directory that
//! holds it reads them too.

use super::{fold, glob};
use crate::record::{ConfigDir, ConfigVariable};

struct Store {
    /// The default directory below HOME.
    dir: &'static str,
    /// Variables that move the directory, in the client's precedence, with
    /// the path each names below its value.
    variables: &'static [(ConfigVariable, &'static str)],
    /// What the client reads from the directory; a trailing `/` marks a
    /// directory of credential files.
    files: &'static [&'static str],
}

const STORES: &[Store] = &[
    Store {
        dir: ".cargo",
        variables: &[(ConfigVariable::CargoHome, "")],
        files: &["credentials", "credentials.toml"],
    },
    Store {
        dir: ".config/gh",
        variables: &[
            (ConfigVariable::GhConfigDir, ""),
            (ConfigVariable::XdgConfigHome, "/gh"),
        ],
        files: &["hosts.yml"],
    },
    // glab reads `GLAB_CONFIG_DIR`, then this legacy directory, then the
    // user configuration directory, `XDG_CONFIG_HOME` when it is set.
    Store {
        dir: ".config/glab-cli",
        variables: &[
            (ConfigVariable::GlabConfigDir, ""),
            (ConfigVariable::XdgConfigHome, "/glab-cli"),
        ],
        files: &["config.yml"],
    },
    Store {
        dir: "Library/Application Support/glab-cli",
        variables: &[],
        files: &["config.yml"],
    },
    Store {
        dir: ".aws",
        variables: &[],
        files: &["credentials"],
    },
    Store {
        dir: ".docker",
        variables: &[],
        files: &["config.json"],
    },
    Store {
        dir: ".kube",
        variables: &[],
        files: &["config"],
    },
    Store {
        dir: ".gnupg",
        variables: &[],
        files: &["private-keys-v1.d/"],
    },
];

#[derive(PartialEq, Eq)]
pub(super) struct Location {
    files: &'static [&'static str],
    /// Folded credential paths.
    credentials: Vec<String>,
}

impl Location {
    fn new(dir: &str, files: &'static [&'static str]) -> Self {
        let dir = fold(dir.trim_end_matches('/'));
        Self {
            files,
            credentials: files
                .iter()
                .map(|file| format!("{dir}/{}", file.trim_end_matches('/')))
                .collect(),
        }
    }

    /// The same store in the directory a link leads to.
    pub(super) fn moved(&self, dir: &str) -> Self {
        Self::new(dir, self.files)
    }
}

/// Each store's default directory under `home`, spelled for a probe.
pub(super) fn defaults(home: &str) -> impl Iterator<Item = (String, Location)> {
    STORES.iter().map(move |store| {
        let dir = format!("{home}/{}", store.dir);
        let location = Location::new(&dir, store.files);
        (dir, location)
    })
}

/// The store directories the hook's environment names; a command inherits it.
pub(super) fn from_environment(dirs: &[ConfigDir]) -> Vec<Location> {
    STORES
        .iter()
        .filter_map(|store| {
            store.variables.iter().find_map(|(variable, below)| {
                let dir = dirs.iter().find(|dir| dir.variable == *variable)?;
                let dir = dir.path.trim_end_matches('/');
                Some(Location::new(&format!("{dir}{below}"), store.files))
            })
        })
        .collect()
}

/// A folded literal path to, or inside, a credential file.
pub(super) fn holds<'a>(mut locations: impl Iterator<Item = &'a Location>, path: &str) -> bool {
    locations.any(|location| {
        location.credentials.iter().any(|file| {
            path.strip_prefix(file.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        })
    })
}

/// A folded pattern whose expansion can name a credential file.
pub(super) fn selects<'a>(
    mut locations: impl Iterator<Item = &'a Location>,
    pattern: &str,
    hidden: bool,
    matcher: &mut glob::Matcher,
) -> bool {
    locations.any(|location| {
        location
            .credentials
            .iter()
            .any(|file| matcher.visible_path(pattern, file, hidden))
    })
}

/// Whether a walk from a folded directory that `root` accepts reaches a
/// credential file through visible directories, as one without `--hidden` does.
/// A walk that folded name bounds narrow reads only the files they match, but
/// every file below a credential directory.
pub(super) fn reached<'a>(
    mut locations: impl Iterator<Item = &'a Location>,
    names: Option<&[String]>,
    mut root: impl FnMut(&str) -> bool,
) -> bool {
    locations.any(|location| {
        location
            .files
            .iter()
            .zip(&location.credentials)
            .any(|(entry, file)| {
                if let Some(names) = names
                    && !entry.ends_with('/')
                    && !names.iter().any(|name| glob::component(name, entry))
                {
                    return false;
                }
                let mut below = file.as_str();
                while let Some((dir, name)) = below.rsplit_once('/')
                    && (below == file || !name.starts_with('.'))
                {
                    if root(dir) {
                        return true;
                    }
                    below = dir;
                }
                false
            })
    })
}
