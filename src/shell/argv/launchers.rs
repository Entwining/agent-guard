use super::{Resolution, text};
use crate::options::{Binding, Grammar, Syntax};
use crate::record::Effect::{Enter, Name, Use};
use crate::record::{Effect, OptionValue, Role, Word};

/// A program form that runs the rest of argv as another command. The option
/// grammars are transcribed from each launcher's own help or source; an
/// option missing from them leaves the launcher as an unknown program. A
/// value naming a file or directory the launcher uses itself takes
/// `Effect::Use`, and one naming the inner command's working directory
/// `Effect::Enter`.
struct Launcher {
    /// Program name followed by the subcommand words that select this form.
    command: &'static [&'static str],
    grammar: Grammar,
    /// Options that take the next word only when it does not start with `-`.
    optional: &'static [&'static str],
    /// Options may follow the subcommand words.
    options_after: bool,
    /// The launcher keeps parsing options after the inner command unless `--`
    /// ended its options.
    interspersed: bool,
    /// The inner command is a Python script the launcher runs when a script
    /// flag precedes it or its name ends in `.py` or `.pyw`.
    scripts: bool,
    /// Operands before a required `--` select what the launcher loads, and
    /// the command follows the `--`.
    selectors: bool,
}

const SCRIPT_FLAGS: &[&str] = &["--script", "-s", "--gui-script"];

// uv 0.13 `uv help run`; global options are accepted on both sides of `run`.
// Package sources, indexes and interpreters may be local paths.
const UV: Launcher = Launcher {
    command: &["uv", "run"],
    grammar: Grammar {
        syntax: Syntax::Clap,
        short: "msUnqv",
        flags: "--all-extras --no-dev --only-dev --no-default-groups --all-groups --module --no-editable --exact --no-env-file --isolated --active --no-sync --locked --frozen --script --gui-script --all-packages --no-project --no-index --upgrade --no-sources --reinstall --compile-bytecode --no-build-isolation --no-build --no-binary --require-build-hashes --no-require-build-hashes --no-cache --refresh --managed-python --no-managed-python --no-python-downloads --quiet --verbose --system-certs --offline --no-progress --no-config --lfs",
        values: &[
            (
                "--with -w --with-editable --index --default-index --index-url -i --extra-index-url --find-links -f --python -p --from --env-file --with-requirements --constraints -c --build-constraints -b --overrides --project --config-file --cache-dir",
                Use,
            ),
            ("--directory", Enter),
            (
                "--extra --no-extra --group --no-group --only-group --no-editable-package --package --python-platform --index-strategy --keyring-provider --upgrade-package -P --upgrade-group --resolution --prerelease --prerelease-package --fork-strategy --exclude-newer --exclude-newer-package --no-sources-package --reinstall-package --link-mode --config-setting -C --config-settings-package --no-build-isolation-package --no-build-package --no-binary-package --refresh-package --color --allow-insecure-host --torch-backend",
                Name,
            ),
        ],
        attached: &[],
        stops: false,
        complete: true,
    },
    optional: &[],
    options_after: true,
    interspersed: false,
    scripts: true,
    selectors: false,
};

// npm 11 `npm exec`; npx alone documents `-p` for `--package` and parses no
// option after the first positional argument. A package spec may be a local
// directory or tarball.
const NPM_GRAMMAR: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "yqs",
    flags: "--yes --no --workspaces --ws --include-workspace-root --prefer-online --prefer-offline --offline --quiet --silent",
    values: &[
        ("--package --workspace -w", Use),
        ("--registry --loglevel", Name),
    ],
    attached: &[],
    stops: false,
    complete: true,
};
const NPM: Launcher = Launcher {
    command: &["npm", "exec"],
    grammar: NPM_GRAMMAR,
    optional: &[],
    options_after: true,
    interspersed: true,
    scripts: false,
    selectors: false,
};

// pnpm `exec` takes its options before the `exec` keyword; `dlx` after it.
// A filter may select packages by directory.
const PNPM: Launcher = Launcher {
    command: &["pnpm", "exec"],
    grammar: Grammar {
        syntax: Syntax::Getopt,
        short: "rws",
        flags: "--recursive --parallel --no-reporter-hide-prefix --report-summary --workspace-root --silent",
        values: &[
            ("--filter", Use),
            ("--dir -C", Enter),
            ("--resume-from --reporter", Name),
        ],
        attached: &[],
        stops: false,
        complete: true,
    },
    optional: &[],
    options_after: false,
    interspersed: false,
    scripts: false,
    selectors: false,
};

const BUN: Launcher = Launcher {
    command: &["bunx"],
    grammar: Grammar {
        syntax: Syntax::Getopt,
        short: "",
        flags: "--bun --no-install --trust --verbose --silent",
        values: &[
            ("--env-file --package -p", Use),
            ("--minimum-release-age", Name),
        ],
        attached: &[],
        stops: false,
        complete: true,
    },
    optional: &[],
    options_after: true,
    interspersed: false,
    scripts: false,
    selectors: false,
};

// 1Password CLI `op run` options, including op's global flags.
const OP: Launcher = Launcher {
    command: &["op", "run"],
    grammar: Grammar {
        syntax: Syntax::Clap,
        short: "h",
        flags: "--no-masking --help --cache --debug --iso-timestamps --no-color",
        values: &[
            ("--env-file --config", Use),
            (
                "--environment --account --encoding --format --session",
                Name,
            ),
        ],
        attached: &[],
        stops: false,
        complete: true,
    },
    optional: &[],
    options_after: true,
    interspersed: true,
    scripts: false,
    selectors: false,
};

// dotenv-cli parses argv with minimist, which gives every undeclared option
// the next word that does not start with `-`. `-p` prints a loaded value and
// stays outside the grammar.
const DOTENV: Launcher = Launcher {
    command: &["dotenv"],
    grammar: Grammar {
        syntax: Syntax::Getopt,
        short: "",
        flags: "--no-expand",
        values: &[("-e", Use), ("-v", Name)],
        attached: &[],
        stops: false,
        complete: true,
    },
    optional: &[
        "-c",
        "--debug",
        "--quiet",
        "-q",
        "-o",
        "--override",
        "--help",
    ],
    options_after: true,
    interspersed: true,
    scripts: false,
    selectors: false,
};

// Maskey's `mk run`: releases before 0.0.3 select entries with `-n` and
// `-e`, later ones with selector operands. Its help forms run no command.
const MK: Launcher = Launcher {
    command: &["mk", "run"],
    grammar: Grammar {
        syntax: Syntax::Clap,
        short: "",
        flags: "--json",
        values: &[("-n --namespace -e --entry", Name)],
        attached: &[],
        stops: false,
        complete: true,
    },
    optional: &[],
    options_after: true,
    interspersed: false,
    scripts: false,
    selectors: true,
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
        grammar: Grammar {
            values: &[
                ("--package -p --workspace -w", Use),
                ("--registry --loglevel", Name),
            ],
            ..NPM_GRAMMAR
        },
        interspersed: false,
        ..NPM
    },
    PNPM,
    Launcher {
        command: &["pnpm", "dlx"],
        grammar: Grammar {
            short: "s",
            flags: "--silent",
            values: &[
                ("--package", Use),
                ("--dir -C", Enter),
                ("--allow-build", Name),
            ],
            ..PNPM.grammar
        },
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
    MK,
];

/// One option word, or an option and its value in the next word. Returns the
/// number of words consumed, or None for an option the grammar does not know
/// or a value word that is not one fixed word.
fn option(launcher: &Launcher, argv: &[Word], at: usize, binding: &mut Binding) -> Option<usize> {
    let word = text(argv, at);
    if let Some((name, _)) = word.split_once('=')
        && word.starts_with("--")
        && launcher.optional.contains(&name)
    {
        return Some(1);
    }
    if launcher.optional.contains(&word) {
        let next = argv.get(at + 1);
        if next.is_some_and(Word::option_unknown) {
            return None;
        }
        let usable = next.is_some_and(|word| {
            word.fixed_position && !word.cardinality_unknown && !word.starts_with('-')
        });
        return Some(if usable { 2 } else { 1 });
    }
    let words = binding.option(argv, at, &launcher.grammar)?;
    binding.unproven.is_none().then_some(words)
}

/// Where the inner command starts, the values of the launcher's options, and
/// its selector operands, found without changing argv.
struct Consumed {
    command: usize,
    binding: Binding,
    selectors: Vec<usize>,
}

fn walk(launcher: &Launcher, argv: &[Word], start: usize) -> Option<Consumed> {
    let mut binding = Binding::default();
    let mut selectors = Vec::new();
    let mut path = &launcher.command[1..];
    let mut at = start + 1;
    let mut ended = false;
    loop {
        // A launcher form without an inner command leaves it to the launcher's
        // own input, such as the items `xargs` appends.
        let word = argv.get(at)?;
        // Text the shell does not fix cannot select a form, and may be an
        // option. Before a required `--` it is taken as a selector, since the
        // launcher's options there select entries and do not move the command.
        if word.option_unknown() || word.cardinality_unknown || word.runtime_unknown {
            if !launcher.selectors || !path.is_empty() {
                return None;
            }
            selectors.push(at);
            at += 1;
            continue;
        }
        if let Some((next, rest)) = path.split_first()
            && word.exact() == Some(*next)
        {
            path = rest;
            at += 1;
            continue;
        }
        let option_word = word.starts_with('-') && word.len() > 1;
        if path.is_empty() && word.exact() == Some("--") {
            ended = true;
            at += 1;
            break;
        }
        // Before the subcommand words only global options may appear; after
        // them, a form that takes no options starts its command here.
        if option_word && selectors.is_empty() && (!path.is_empty() || launcher.options_after) {
            at += option(launcher, argv, at, &mut binding)?;
            continue;
        }
        if option_word || !path.is_empty() {
            return None;
        }
        if !launcher.selectors {
            break;
        }
        selectors.push(at);
        at += 1;
    }
    // An unexpanded directory would leave the inner command's cwd unknown.
    if binding
        .values
        .iter()
        .any(|bound| bound.value.effect == Enter && argv[bound.value.index].expands)
    {
        return None;
    }
    argv.get(at)?;
    if launcher.interspersed
        && !ended
        && argv[at..]
            .iter()
            .any(|word| word.option_unknown() || word.starts_with('-') && word.len() > 1)
    {
        return None;
    }
    Some(Consumed {
        command: at,
        binding,
        selectors,
    })
}

pub(super) fn resolve(
    result: &mut Resolution,
    name: &str,
    argv: &mut [Word],
    index: &mut usize,
) -> bool {
    let Some((launcher, found)) = LAUNCHERS
        .iter()
        .filter(|launcher| launcher.command[0] == name)
        .find_map(|launcher| Some((launcher, walk(launcher, argv, *index)?)))
    else {
        return false;
    };
    // The launcher resolves its own file options in the directory it enters.
    let entered = found
        .binding
        .values
        .iter()
        .rev()
        .find(|bound| bound.value.effect == Enter)
        .map(|bound| bound.value.clone());
    let files = found
        .binding
        .values
        .iter()
        .filter(|bound| bound.value.effect == Use)
        .map(|bound| bound.value.clone());
    // A script the launcher runs itself is read like an interpreter's script.
    let script = (launcher.scripts
        && (SCRIPT_FLAGS.iter().any(|flag| found.binding.has(flag))
            || [".py", ".pyw"]
                .iter()
                .any(|suffix| argv[found.command].ends_with(suffix))))
    .then_some(OptionValue {
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
    for &selector in &found.selectors {
        argv[selector].role = Role::Namespace;
    }
    *index = found.command;
    true
}
