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

/// A row admits a file or directory that a client's documentation or source
/// names as its credential storage, fallback storage included, where agents
/// reach it in ordinary work.
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
        variables: &[(ConfigVariable::DockerConfig, "")],
        files: &["config.json"],
    },
    Store {
        dir: ".kube",
        variables: &[],
        files: &["config"],
    },
    Store {
        dir: ".gnupg",
        variables: &[(ConfigVariable::Gnupghome, "")],
        files: &["private-keys-v1.d/"],
    },
    Store {
        dir: ".pi/agent",
        variables: &[(ConfigVariable::PiCodingAgentDir, "")],
        files: &["auth.json", "mcp-auth.json"],
    },
    // `.credentials.json` holds MCP OAuth tokens where no keyring is available.
    Store {
        dir: ".codex",
        variables: &[(ConfigVariable::CodexHome, "")],
        files: &["auth.json", ".credentials.json"],
    },
    // Claude Code's fallback where the macOS Keychain refuses the login.
    Store {
        dir: ".claude",
        variables: &[(ConfigVariable::ClaudeConfigDir, "")],
        files: &[".credentials.json"],
    },
    // Snowflake CLI falls back to Application Support when the directory
    // `SNOWFLAKE_HOME` or `~/.snowflake` names is missing.
    Store {
        dir: ".snowflake",
        variables: &[(ConfigVariable::SnowflakeHome, "")],
        files: &["config.toml", "connections.toml"],
    },
    Store {
        dir: "Library/Application Support/snowflake",
        variables: &[],
        files: &["config.toml", "connections.toml"],
    },
    // dbt v1 reads `DBT_ENGINE_PROFILES_DIR` before the legacy variable; a
    // project's own `profiles.yml` is not a store.
    Store {
        dir: ".dbt",
        variables: &[
            (ConfigVariable::DbtEngineProfilesDir, ""),
            (ConfigVariable::DbtProfilesDir, ""),
        ],
        files: &["profiles.yml"],
    },
    // `pnpm login` writes `auth.ini`, or `_auth` in the global `config.yaml`
    // in later releases, which still read `auth.ini`; the global `rc` holds
    // registry and auth settings.
    Store {
        dir: "Library/Preferences/pnpm",
        variables: &[(ConfigVariable::XdgConfigHome, "/pnpm")],
        files: &["auth.ini", "config.yaml", "rc"],
    },
];

#[derive(PartialEq, Eq)]
pub(super) struct Location {
    /// The folded store directory.
    dir: String,
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
            dir,
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

/// Whether moving the store directory at folded `source` to folded
/// `destination` lands every store it carries where a store with the same
/// credential files is located, so their reads and walks stay protected.
pub(super) fn keeps_stores(locations: &[&Location], source: &str, destination: &str) -> bool {
    let mut carried = locations.iter().filter_map(|location| {
        location
            .dir
            .strip_prefix(source)
            .filter(|rest| rest.is_empty() || rest.starts_with('/'))
            .map(|rest| (location, format!("{destination}{rest}")))
    });
    locations.iter().any(|location| location.dir == source)
        && carried.all(|(carried, moved)| {
            locations.iter().any(|location| {
                location.dir == moved
                    && carried
                        .files
                        .iter()
                        .all(|file| location.files.contains(file))
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
/// credential file. A walk that folded name bounds narrow reads only the files
/// they match, but every file below a credential directory. Unless it enters
/// hidden names, it passes a dot name only where a bound matches it, as
/// ripgrep does without `--hidden`.
pub(super) fn reached<'a>(
    mut locations: impl Iterator<Item = &'a Location>,
    names: Option<&[String]>,
    hidden: bool,
    mut root: impl FnMut(&str) -> bool,
) -> bool {
    let enters = |name: &str| {
        hidden
            || !name.starts_with('.')
            || names.is_some_and(|names| names.iter().any(|bound| glob::component(bound, name)))
    };
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
                    && enters(name)
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
