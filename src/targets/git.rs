use super::*;
use crate::options::{Grammar, Syntax};
use crate::record::Effect::{Name, Read, Write};

mod pathspec;

pub(super) fn infer_git(args: &[Word], cwd: &str, host: HostFacts<'_>, effects: &mut Effects) {
    let (index, base) = global_options(args, cwd, host, effects);
    infer_subcommand(args, index, &base, cwd, host, effects);
}

fn infer_subcommand(
    args: &[Word],
    mut index: usize,
    base: &str,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    let Some(sub) = args.get(index) else {
        return;
    };
    if sub.exact().is_none() {
        effects.unproven.get_or_insert(index);
    }
    index += 1;
    // Credential fill exposes stored secrets without naming a file to read.
    effects.stored_secret |=
        sub == "credential" && args.get(index).is_some_and(|arg| arg == "fill");
    let names="branch tag remote switch push fetch pull merge rebase cherry-pick revert reflog rev-parse describe bisect init clone submodule worktree config lfs sparse-checkout".split_whitespace().any(|name|name==sub.as_str());
    let metadata="add rm mv restore checkout reset stash check-ignore check-attr update-index ls-files status clean commit".split_whitespace().any(|name|name==sub.as_str());
    let pathspec = !names && sub != "grep";
    let add = |path: &str, word: &Word, effect: Effect, walk: Walk, effects: &mut Effects| {
        let glob = pathspec && path.contains(['*', '?', '[']);
        let mut target = Target::from_word(
            &word.with_text(crate::filesystem::normalize(path, base, "")),
            cwd,
            host,
            effect,
            walk,
        );
        target.glob = glob;
        effects.targets.push(target);
        if effect == Effect::Read
            && let Some((_, path)) = path.split_once(':')
        {
            let mut target = Target::from_word(
                &word.with_text(crate::filesystem::normalize(path, base, "")),
                cwd,
                host,
                effect,
                Walk::None,
            );
            target.glob = glob;
            effects.targets.push(target);
        }
    };
    let binding = crate::options::bind(&args[index..], subcommand_options(sub.as_str()));
    for bound in &binding.values {
        let value = bound.value.word(&args[index..]);
        match bound.value.effect {
            Effect::Name => {}
            Effect::Read => add(&value.text, &value, Effect::Read, Walk::None, effects),
            effect => {
                effects
                    .targets
                    .push(Target::from_word(&value, base, host, effect, Walk::None))
            }
        }
    }
    if let Some(start) = binding.unproven {
        effects.unproven.get_or_insert(index + start);
    }
    // Patterns given as option values leave every operand a pathspec.
    let mut pattern = sub != "grep"
        || binding
            .values
            .iter()
            .any(|bound| ["-e", "-f"].contains(&bound.name));
    let mut bundle_action = None;
    let mut bundle_output = false;
    for operand in binding.operands {
        let arg = &args[index + operand];
        // After a word the grammar cannot place, no operand becomes the grep
        // pattern.
        pattern |= binding.unproven.is_some_and(|start| start <= operand);
        // A bundle action is data; only create's first file is an output.
        if sub == "bundle" && bundle_action.is_none() {
            bundle_action = Some(arg.text.clone());
            continue;
        }
        if !pattern {
            pattern = true;
            continue;
        }
        let output =
            sub == "bundle" && bundle_action.as_deref() == Some("create") && !bundle_output;
        bundle_output |= output;
        let (effect, walk) = if output {
            (Effect::Write, Walk::None)
        } else if metadata {
            (Effect::Meta, Walk::None)
        } else if names {
            (Effect::Name, Walk::None)
        } else {
            (Effect::Read, Walk::Visible)
        };
        match pathspec::parse(arg) {
            // An exclusion only removes paths from the selected set.
            Some(magic) if magic.exclude => {}
            Some(magic) => add(magic.path, arg, effect, walk, effects),
            None => add(arg, arg, effect, walk, effects),
        }
    }
}

fn global_options(
    args: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) -> (usize, String) {
    let mut index = 0;
    let mut base = cwd.to_owned();
    let option_walk = if args.iter().any(|arg| arg == "config") {
        Walk::None
    } else {
        Walk::Visible
    };
    while let Some(arg) = args.get(index).filter(|arg| arg.starts_with('-')) {
        if effects.unproven.is_none() && arg.option_unknown() {
            effects.unproven = Some(index);
        }
        let takes = effects.unproven.is_none()
            && [
                "-C",
                "-c",
                "--git-dir",
                "--work-tree",
                "--namespace",
                "--exec-path",
            ]
            .contains(&arg.as_str());
        let path = arg
            .strip_prefix("--work-tree=")
            .map(|text| arg.with_text(text.to_owned()))
            .or_else(|| {
                if takes && ["-C", "--work-tree"].contains(&arg.as_str()) {
                    args.get(index + 1).cloned()
                } else {
                    None
                }
            });
        if let Some(path) = path {
            base = crate::filesystem::normalize(&path, &base, "");
            effects.targets.push(Target::from_word(
                &path.with_text(base.clone()),
                cwd,
                host,
                Effect::Enter,
                option_walk,
            ));
        }
        // Repository-directory values retain the read role at the command cwd,
        // independently of the work-tree base used for pathspecs.
        let directory = arg
            .strip_prefix("--git-dir=")
            .map(|text| arg.with_text(text.to_owned()))
            .or_else(|| {
                (takes && arg == "--git-dir")
                    .then(|| args.get(index + 1).cloned())
                    .flatten()
            });
        if let Some(directory) = directory {
            effects.targets.push(Target::from_word(
                &directory,
                cwd,
                host,
                Effect::Read,
                option_walk,
            ));
        }
        if takes
            && arg == "-c"
            && let Some(value) = args.get(index + 1)
        {
            // Configuration values retain the generic operand's read role.
            effects.targets.push(Target::from_word(
                value,
                cwd,
                host,
                Effect::Read,
                option_walk,
            ));
        }
        index += if takes { 2 } else { 1 };
    }
    (index, base)
}

/// The value options of a subcommand whose values are not data, and of those
/// whose data values agents pass in separate words. Git's parse-options binds
/// `-xVALUE`, `-x VALUE`, `--name=VALUE` and `--name VALUE`; other options
/// keep the operand roles.
fn subcommand_options(sub: &str) -> &'static Grammar {
    match sub {
        "config" => &const { Grammar::partial(Syntax::Getopt, &[("-f --file --blob", Read)]) },
        "blame" => &const { Grammar::partial(Syntax::Getopt, &[("--contents", Read)]) },
        "commit" => {
            &const {
                Grammar::partial(
                    Syntax::Getopt,
                    &[
                        ("-F --file --pathspec-from-file -t --template", Read),
                        ("-m --message", Name),
                    ],
                )
            }
        }
        "tag" | "merge" => {
            &const {
                Grammar::partial(
                    Syntax::Getopt,
                    &[("-F --file", Read), ("-m --message", Name)],
                )
            }
        }
        "add" | "rm" | "restore" | "reset" | "checkout" | "stash" => {
            &const { Grammar::partial(Syntax::Getopt, &[("--pathspec-from-file", Read)]) }
        }
        "grep" => {
            &const {
                Grammar::partial(
                    Syntax::Getopt,
                    &[
                        ("-f", Read),
                        (
                            "-e -A --after-context -B --before-context -C --context --max-depth -m --max-count --threads",
                            Name,
                        ),
                    ],
                )
            }
        }
        "diff" | "log" | "show" | "whatchanged" | "diff-tree" | "diff-index" | "diff-files"
        | "range-diff" => &const { Grammar::partial(Syntax::Getopt, &[("--output", Write)]) },
        "archive" => {
            &const {
                Grammar::partial(
                    Syntax::Getopt,
                    &[
                        ("-o --output", Write),
                        ("--add-file", Read),
                        ("--prefix --format --remote --exec", Name),
                    ],
                )
            }
        }
        "format-patch" => {
            &const {
                Grammar::partial(
                    Syntax::Getopt,
                    &[
                        ("-o --output-directory --output", Write),
                        ("--signature-file", Read),
                    ],
                )
            }
        }
        _ => &const { Grammar::partial(Syntax::Getopt, &[]) },
    }
}
