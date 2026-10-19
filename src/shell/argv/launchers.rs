use super::{Resolution, text};
use crate::record::{Effect, Role, Word, WrapperFile};

/// A program form that runs the rest of argv as another command. The option
/// tables are transcribed from each launcher's own help or source; an option
/// missing from them leaves the launcher as an unknown program.
struct Launcher {
    /// Program name followed by the subcommand words that select this form.
    command: &'static [&'static str],
    flags: &'static [&'static str],
    values: &'static [&'static str],
    /// Options whose value can name a file or directory the launcher uses
    /// itself.
    files: &'static [&'static str],
    /// Options that take the next word only when it does not start with `-`.
    optional: &'static [&'static str],
    /// Options whose value becomes the working directory of the inner command.
    directory: &'static [&'static str],
    /// Options may follow the subcommand words.
    options_after: bool,
    /// The launcher keeps parsing options after the inner command unless `--`
    /// ended its options.
    interspersed: bool,
    /// The inner command is a Python script the launcher runs when a script
    /// flag precedes it or its name ends in `.py` or `.pyw`.
    scripts: bool,
}

const SCRIPT_FLAGS: &[&str] = &["--script", "-s", "--gui-script"];

// uv 0.13 `uv help run`; global options are accepted on both sides of `run`.
const UV_FLAGS: &[&str] = &[
    "--all-extras",
    "--no-dev",
    "--only-dev",
    "--no-default-groups",
    "--all-groups",
    "--module",
    "-m",
    "--no-editable",
    "--exact",
    "--no-env-file",
    "--isolated",
    "--active",
    "--no-sync",
    "--locked",
    "--frozen",
    "--script",
    "-s",
    "--gui-script",
    "--all-packages",
    "--no-project",
    "--no-index",
    "--upgrade",
    "-U",
    "--no-sources",
    "--reinstall",
    "--compile-bytecode",
    "--no-build-isolation",
    "--no-build",
    "--no-binary",
    "--require-build-hashes",
    "--no-require-build-hashes",
    "--no-cache",
    "-n",
    "--refresh",
    "--managed-python",
    "--no-managed-python",
    "--no-python-downloads",
    "--quiet",
    "-q",
    "--verbose",
    "-v",
    "--system-certs",
    "--offline",
    "--no-progress",
    "--no-config",
    "--lfs",
];
const UV_VALUES: &[&str] = &[
    "--extra",
    "--no-extra",
    "--group",
    "--no-group",
    "--only-group",
    "--no-editable-package",
    "--package",
    "--python-platform",
    "--index-strategy",
    "--keyring-provider",
    "--upgrade-package",
    "-P",
    "--upgrade-group",
    "--resolution",
    "--prerelease",
    "--prerelease-package",
    "--fork-strategy",
    "--exclude-newer",
    "--exclude-newer-package",
    "--no-sources-package",
    "--reinstall-package",
    "--link-mode",
    "--config-setting",
    "-C",
    "--config-settings-package",
    "--no-build-isolation-package",
    "--no-build-package",
    "--no-binary-package",
    "--refresh-package",
    "--color",
    "--allow-insecure-host",
    "--torch-backend",
];
// Package sources, indexes and interpreters may be local paths.
const UV_FILES: &[&str] = &[
    "--with",
    "-w",
    "--with-editable",
    "--index",
    "--default-index",
    "--index-url",
    "-i",
    "--extra-index-url",
    "--find-links",
    "-f",
    "--python",
    "-p",
    "--from",
    "--env-file",
    "--with-requirements",
    "--constraints",
    "-c",
    "--build-constraints",
    "-b",
    "--overrides",
    "--project",
    "--config-file",
    "--cache-dir",
];
const UV: Launcher = Launcher {
    command: &["uv", "run"],
    flags: UV_FLAGS,
    values: UV_VALUES,
    files: UV_FILES,
    optional: &[],
    directory: &["--directory"],
    options_after: true,
    interspersed: false,
    scripts: true,
};

// npm 11 `npm exec`; npx alone documents `-p` for `--package` and parses no
// option after the first positional argument.
const NPM_FLAGS: &[&str] = &[
    "--yes",
    "-y",
    "--no",
    "--workspaces",
    "--ws",
    "--include-workspace-root",
    "--prefer-online",
    "--prefer-offline",
    "--offline",
    "--quiet",
    "-q",
    "--silent",
    "-s",
];
const NPM_VALUES: &[&str] = &["--registry", "--loglevel"];
// A package spec may be a local directory or tarball.
const NPM_FILES: &[&str] = &["--package", "--workspace", "-w"];
const NPX_FILES: &[&str] = &["--package", "-p", "--workspace", "-w"];
const NPM: Launcher = Launcher {
    command: &["npm", "exec"],
    flags: NPM_FLAGS,
    values: NPM_VALUES,
    files: NPM_FILES,
    optional: &[],
    directory: &[],
    options_after: true,
    interspersed: true,
    scripts: false,
};

// pnpm `exec` takes its options before the `exec` keyword; `dlx` after it.
const PNPM_FLAGS: &[&str] = &[
    "--recursive",
    "-r",
    "--parallel",
    "--no-reporter-hide-prefix",
    "--report-summary",
    "--workspace-root",
    "-w",
    "--silent",
    "-s",
];
const PNPM_VALUES: &[&str] = &["--resume-from", "--reporter"];
const PNPM: Launcher = Launcher {
    command: &["pnpm", "exec"],
    flags: PNPM_FLAGS,
    values: PNPM_VALUES,
    // A filter may select packages by directory.
    files: &["--filter"],
    optional: &[],
    directory: &["--dir", "-C"],
    options_after: false,
    interspersed: false,
    scripts: false,
};

const BUN: Launcher = Launcher {
    command: &["bunx"],
    flags: &["--bun", "--no-install", "--trust", "--verbose", "--silent"],
    values: &["--minimum-release-age"],
    files: &["--env-file", "--package", "-p"],
    optional: &[],
    directory: &[],
    options_after: true,
    interspersed: false,
    scripts: false,
};

// 1Password CLI `op run` options, including op's global flags.
const OP: Launcher = Launcher {
    command: &["op", "run"],
    flags: &[
        "--no-masking",
        "--help",
        "-h",
        "--cache",
        "--debug",
        "--iso-timestamps",
        "--no-color",
    ],
    values: &[
        "--environment",
        "--account",
        "--encoding",
        "--format",
        "--session",
    ],
    files: &["--env-file", "--config"],
    optional: &[],
    directory: &[],
    options_after: true,
    interspersed: true,
    scripts: false,
};

// dotenv-cli parses argv with minimist, which gives every undeclared option
// the next word that does not start with `-`. `-p` prints a loaded value and
// stays outside the table.
const DOTENV: Launcher = Launcher {
    command: &["dotenv"],
    flags: &["--no-expand"],
    values: &["-v"],
    files: &["-e"],
    optional: &[
        "-c",
        "--debug",
        "--quiet",
        "-q",
        "-o",
        "--override",
        "--help",
    ],
    directory: &[],
    options_after: true,
    interspersed: true,
    scripts: false,
};

const LAUNCHERS: &[Launcher] = &[
    UV,
    Launcher {
        command: &["uv", "tool", "run"],
        scripts: false,
        ..UV
    },
    Launcher {
        command: &["uvx"],
        scripts: false,
        ..UV
    },
    NPM,
    Launcher {
        command: &["npm", "x"],
        ..NPM
    },
    Launcher {
        command: &["npx"],
        files: NPX_FILES,
        interspersed: false,
        ..NPM
    },
    PNPM,
    Launcher {
        command: &["pnpm", "dlx"],
        flags: &["--silent", "-s"],
        values: &["--allow-build"],
        files: &["--package"],
        options_after: true,
        ..PNPM
    },
    BUN,
    Launcher {
        command: &["bun", "x"],
        ..BUN
    },
    OP,
    DOTENV,
];

#[derive(Clone, Copy)]
enum Kind {
    Flag,
    Value,
    File,
    Optional,
    Directory,
}

impl Launcher {
    fn kind(&self, option: &str) -> Option<Kind> {
        if self.flags.contains(&option) {
            Some(Kind::Flag)
        } else if self.values.contains(&option) {
            Some(Kind::Value)
        } else if self.files.contains(&option) {
            Some(Kind::File)
        } else if self.optional.contains(&option) {
            Some(Kind::Optional)
        } else if self.directory.contains(&option) {
            Some(Kind::Directory)
        } else {
            None
        }
    }
}

/// The words a launcher consumes, found without changing argv. Option values
/// are argv indexes with the length of a glued `--option=` prefix.
#[derive(Default)]
struct Consumed {
    command: usize,
    files: Vec<(usize, usize)>,
    directory: Option<(usize, usize)>,
    script: bool,
}

/// One option word, or an option and its value in the next word. Returns the
/// number of words consumed, or None for an option the table does not know or
/// a value word that is not one fixed word.
fn option(launcher: &Launcher, argv: &[Word], at: usize, consumed: &mut Consumed) -> Option<usize> {
    let word = text(argv, at);
    let next = argv.get(at + 1);
    let next_usable = next.is_some_and(|word| word.fixed_position && !word.cardinality_unknown);
    let (name, inline) = if word.starts_with("--") {
        word.split_once('=')
            .map_or((word, false), |(name, _)| (name, true))
    } else if word.chars().count() == 2 {
        (word, false)
    } else {
        // Short clusters and glued short values are left to the fallback.
        return None;
    };
    consumed.script |= launcher.scripts && SCRIPT_FLAGS.contains(&name);
    match launcher.kind(name)? {
        Kind::Flag => Some(1),
        Kind::Optional if inline => Some(1),
        Kind::Optional => Some(
            if next_usable && !next.is_some_and(|w| w.starts_with('-')) {
                2
            } else {
                1
            },
        ),
        kind => {
            let (value, words) = if inline {
                ((at, name.len() + 1), 1)
            } else if next_usable {
                ((at + 1, 0), 2)
            } else {
                return None;
            };
            match kind {
                // An unexpanded directory would leave the inner command's cwd
                // unknown.
                Kind::Directory if argv[value.0].expands => return None,
                Kind::Directory => consumed.directory = Some(value),
                Kind::File => consumed.files.push(value),
                Kind::Flag | Kind::Value | Kind::Optional => {}
            }
            Some(words)
        }
    }
}

fn walk(launcher: &Launcher, argv: &[Word], start: usize) -> Option<Consumed> {
    let mut consumed = Consumed::default();
    let mut path = &launcher.command[1..];
    let mut at = start + 1;
    let mut ended = false;
    loop {
        // A launcher form without an inner command leaves it to the launcher's
        // own input, such as the items `xargs` appends.
        let word = argv.get(at)?;
        if !word.fixed_position || word.cardinality_unknown || word.runtime_unknown {
            return None;
        }
        if let Some((next, rest)) = path.split_first()
            && word == *next
        {
            path = rest;
            at += 1;
            continue;
        }
        let option_word = word.starts_with('-') && word.len() > 1;
        if path.is_empty() && word == "--" {
            ended = true;
            at += 1;
            break;
        }
        // Before the subcommand words only global options may appear; after
        // them, a form that takes no options starts its command here.
        if option_word && (!path.is_empty() || launcher.options_after) {
            at += option(launcher, argv, at, &mut consumed)?;
            continue;
        }
        if option_word || !path.is_empty() {
            return None;
        }
        break;
    }
    let command = argv.get(at)?;
    consumed.command = at;
    consumed.script |= launcher.scripts
        && [".py", ".pyw"]
            .iter()
            .any(|suffix| command.ends_with(suffix));
    if launcher.interspersed
        && !ended
        && argv[at..]
            .iter()
            .any(|word| word.starts_with('-') && word.len() > 1)
    {
        return None;
    }
    Some(consumed)
}

pub(super) fn resolve(
    result: &mut Resolution,
    name: &str,
    argv: &mut [Word],
    index: &mut usize,
) -> bool {
    let Some(found) = LAUNCHERS
        .iter()
        .filter(|launcher| launcher.command[0] == name)
        .find_map(|launcher| walk(launcher, argv, *index))
    else {
        return false;
    };
    // The launcher resolves its own file options in the directory it enters.
    let entered = found.directory.map(|(index, prefix)| WrapperFile {
        index,
        prefix,
        effect: Effect::Enter,
    });
    let files = found.files.into_iter().map(|(index, prefix)| WrapperFile {
        index,
        prefix,
        effect: Effect::Use,
    });
    // A script the launcher runs itself is read like an interpreter's script.
    let script = found.script.then_some(WrapperFile {
        index: found.command,
        prefix: 0,
        effect: Effect::Read,
    });
    result
        .wrapper_files
        .extend(entered.into_iter().chain(files).chain(script));
    for word in &mut argv[*index..found.command] {
        word.role = Role::Precommand;
    }
    *index = found.command;
    true
}
