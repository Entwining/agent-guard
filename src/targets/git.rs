use super::*;

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
    let keys = file_options(sub);
    let mut pattern = sub != "grep";
    let mut options = true;
    let mut bundle_action = None;
    let mut bundle_output = false;
    while index < args.len() {
        let arg = &args[index];
        if options && effects.unproven.is_none() && arg.option_unknown() {
            effects.unproven = Some(index);
        }
        // After a word the grammar cannot place, no option consumes a later
        // word and no operand becomes the grep pattern.
        let scan = effects.unproven.is_some();
        pattern |= scan;
        if options && !scan && sub == "commit" && ["-m", "--message"].contains(&arg.as_str()) {
            index += 2;
            continue;
        }
        let mut option_path = None;
        if sub == "grep" && options {
            let (skip, path) =
                grep_option_path(args, arg, &mut index, &mut options, &mut pattern, scan);
            if skip {
                continue;
            }
            option_path = path;
        }
        option_path = option_path_for_keys(args, arg, &mut index, keys, scan).or(option_path);
        if let Some(path) = option_path {
            add(&path, &path, Effect::Read, Walk::None, effects);
        } else if !options || !arg.starts_with('-') {
            // A bundle action is data; only create's first file is an output.
            if sub == "bundle" && bundle_action.is_none() {
                bundle_action = Some(arg.text.clone());
                index += 1;
                continue;
            }
            if !pattern {
                pattern = true;
                index += 1;
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
        index += 1;
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

/// The options of a subcommand whose value is a file git reads.
fn file_options(sub: &str) -> &'static [&'static str] {
    match sub {
        "config" => &["-f", "--file", "--blob"],
        "blame" => &["--contents"],
        "commit" => &["-F", "--file", "--pathspec-from-file"],
        "tag" | "merge" => &["-F", "--file"],
        "add" | "rm" | "restore" | "reset" | "checkout" | "stash" => &["--pathspec-from-file"],
        _ => &[],
    }
}

fn option_path_for_keys(
    args: &[Word],
    arg: &Word,
    index: &mut usize,
    keys: &[&str],
    scan: bool,
) -> Option<Word> {
    for key in keys {
        if arg == key && !scan {
            *index += 1;
            return args.get(*index).cloned();
        }
        if let Some(path) = arg.strip_prefix(&format!("{key}=")) {
            return Some(arg.with_text(path.to_owned()));
        }
        if !key.starts_with("--")
            && let Some(path) = arg.strip_prefix(key).filter(|path| !path.is_empty())
        {
            return Some(arg.with_text(path.to_owned()));
        }
    }
    None
}

fn grep_option_path(
    args: &[Word],
    arg: &Word,
    index: &mut usize,
    options: &mut bool,
    pattern: &mut bool,
    scan: bool,
) -> (bool, Option<Word>) {
    let mut option_path = None;
    if !scan && arg == "--" {
        *options = false;
        *index += 1;
        return (true, None);
    }
    if !scan && arg == "-e" {
        *pattern = true;
        *index += 2;
        return (true, None);
    }
    if let Some(path) = arg
        .strip_prefix("--file=")
        .or_else(|| arg.strip_prefix("-f="))
    {
        option_path = Some(arg.with_text(path.to_owned()));
    } else if arg == "-f" && !scan {
        *pattern = true;
        *index += 1;
        option_path = args.get(*index).cloned();
    }
    (false, option_path)
}
