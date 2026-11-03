use super::*;

// Builtin commands and aliases of cargo 1.99. Any other word names an
// external subcommand or a configured alias, which stays an unknown program.
const COMMANDS: &[&str] = &[
    "add",
    "b",
    "bench",
    "build",
    "c",
    "check",
    "clean",
    "config",
    "d",
    "doc",
    "fetch",
    "fix",
    "generate-lockfile",
    "git-checkout",
    "help",
    "info",
    "init",
    "install",
    "locate-project",
    "login",
    "logout",
    "metadata",
    "new",
    "owner",
    "package",
    "pkgid",
    "publish",
    "r",
    "read-manifest",
    "remove",
    "report",
    "rm",
    "run",
    "rustc",
    "rustdoc",
    "search",
    "t",
    "test",
    "tree",
    "uninstall",
    "update",
    "vendor",
    "verify-project",
    "version",
    "yank",
];
const GLOBAL_VALUES: &[&str] = &["--explain", "--color", "-C", "--config", "-Z"];
// Every other option value is data, like the package, target and feature
// names that make up cargo's operands.
const OPTIONS: Grammar = Grammar::partial(
    Syntax::Clap,
    &[
        ("--manifest-path -m --lockfile-path --path --sync", Read),
        ("--root --target-dir --artifact-dir", Write),
        // Git sources, registry indexes and target specifications may be paths.
        ("--config --git --index --target", Use),
        ("-C", Effect::Enter),
    ],
);

/// The builtin subcommand, Some(None) when no subcommand is given, or None for
/// an external subcommand, an alias, or runtime text, which could name any
/// subcommand or option.
fn subcommand(words: &[Word]) -> Option<Option<&str>> {
    let toolchain = words
        .first()
        .is_some_and(|word| word.known_prefix().starts_with('+'));
    let mut index = usize::from(toolchain);
    while let Some(word) = words.get(index) {
        let word = word.exact()?;
        if !word.starts_with('-') {
            return COMMANDS.contains(&word).then_some(Some(word));
        }
        index += if GLOBAL_VALUES.contains(&word) { 2 } else { 1 };
    }
    Some(None)
}

pub(super) fn spec(words: &[Word]) -> Option<Spec> {
    let operand = match subcommand(words)? {
        Some("new" | "init" | "vendor") => Effect::Write,
        // `cargo run` passes its operands to the program it runs.
        Some("run" | "r") => Effect::Read,
        _ => Effect::Name,
    };
    Some(Spec {
        operand,
        // Cargo reads the package trees its path options name.
        walk: Walk::Visible,
        options: Some(&OPTIONS),
        remote: false,
        destination: None,
    })
}

/// Words after `--`, and the operands of `cargo run`, go to the program cargo
/// builds or runs.
pub(super) fn cargo(context: &mut Context<'_>, effects: &mut Effects) {
    let runs = matches!(subcommand(context.words), Some(Some("run" | "r")));
    let separator = context.words.iter().position(|word| word == "--");
    if let Some(separator) = separator {
        context.claimed[separator] = true;
        for index in separator + 1..context.words.len() {
            context.claimed[index] = true;
            let word = &context.words[index];
            for value in operand_values(word, false) {
                context.targets.push(Target::from_word(
                    &word.with_text(value.into()),
                    context.cwd,
                    context.host,
                    Effect::Read,
                    Walk::Visible,
                ));
            }
        }
    }
    if runs || separator.is_some() {
        effects.gaps.push(CoverageGap::UnknownProgram {
            program: "cargo".into(),
        });
    }
}
