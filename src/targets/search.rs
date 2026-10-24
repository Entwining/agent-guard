use super::*;

pub(super) fn infer_search(
    program: &str,
    args: &[Word],
    command: &CommandRecord,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    let (mut operands, glob_words, hidden, names, recursive) =
        search_arguments(program, args, cwd, host, effects);
    let (globs_enter_hidden, filters) = glob_filters(program, hidden, &glob_words);
    let reaches_hidden = program == "grep" || hidden || globs_enter_hidden;
    let pathless = operands.is_empty()
        && !(program == "rg" && searches_input(command))
        && (program != "grep" || hidden);
    let items = pathless && xargs_appends(command);
    let implicit = pathless && !items;
    if pathless {
        operands.push(Word::literal(cwd.to_owned()));
    }
    effects.hidden_listing = names && hidden;
    let hidden_walk = recursive || matches!(program, "rg" | "ag") && hidden;
    effects.hidden_content = hidden && !names && !hidden_walk;
    let walk = if hidden_walk && !names {
        Walk::Hidden
    } else if program != "grep" || hidden {
        Walk::Visible
    } else {
        Walk::None
    };
    // Grep and ripgrep read only files that match some positive filter, so
    // the filters bound a walk when every one of them bounds the basename. A
    // glob the shell fills in at run time selects unknown names, and grep
    // matches a `..` glob against the operand's path.
    let positive: Vec<_> = glob_words.iter().filter(|glob| !negated(glob)).collect();
    let bounds = (walk != Walk::None
        && !names
        && positive
            .iter()
            .all(|glob| !glob.expands && !glob.runtime_unknown))
    .then(|| crate::filesystem::filter_bounds(positive.iter().map(|glob| glob.text.as_str())))
    .flatten();
    for root in operands {
        let mut target = Target::from_word(
            &root,
            cwd,
            host,
            if names && !hidden_walk {
                Effect::List
            } else {
                Effect::Read
            },
            walk,
        );
        target.search = implicit && !args.iter().any(|arg| arg == "--help" || arg == "-h");
        if items {
            target.via = Via::Items;
        }
        // ripgrep and ag skip hidden names unless `--hidden` makes the walk
        // hidden or a positive glob matches the name. Bounds decide which
        // names a glob enters; without them, a glob such as `*` enters any.
        target.walk_hidden = walk == Walk::Hidden
            || !matches!(program, "rg" | "ag")
            || bounds.is_none() && globs_enter_hidden;
        if walk == Walk::Hidden {
            target.narrowed = bounds
                .as_ref()
                .map(|names| narrowed_reads(&root, names, cwd, host));
        }
        target.bounds.clone_from(&bounds);
        effects.targets.push(target);
        if !names {
            for glob in &filters {
                effects.targets.push(filter_target(
                    &root,
                    glob,
                    Via::Filter,
                    reaches_hidden,
                    cwd,
                    host,
                ));
            }
        }
    }
}

/// The globs that select files below a search root, with each file one glob
/// selects repeated below the hidden directories another glob enters, and
/// whether any glob enters a hidden name.
fn glob_filters(program: &str, hidden: bool, glob_words: &[Word]) -> (bool, Vec<String>) {
    // A negated glob only excludes, and globs match paths relative to the
    // search root, which never contain `..`; neither selects a file.
    let selecting: Vec<_> = glob_words
        .iter()
        .filter(|glob| !negated(glob) && !glob.split('/').any(|part| part == ".."))
        .map(|glob| &glob.text)
        .collect();
    // A selecting glob the shell fills in at run time may enter any name.
    let unknown = glob_words
        .iter()
        .any(|glob| !negated(glob) && (glob.expands || glob.runtime_unknown));
    // ripgrep skips a hidden entry unless `--hidden` is given or a positive
    // glob matches that entry, so hidden names are reachable only when some
    // glob's last component can match a dot name.
    let entering: Vec<_> = selecting
        .iter()
        .filter(|glob| {
            glob.rsplit('/')
                .next()
                .is_some_and(|last| last.starts_with(['.', '*', '?', '[', '{']))
        })
        .collect();
    let mut filters: Vec<_> = selecting.iter().map(|glob| (*glob).clone()).collect();
    if program != "grep" && !hidden {
        // A file that one glob selects is searched below a hidden directory
        // that another glob enters, as `.aws` does for `credentials`. The
        // entered directories are the dot names the last component matches,
        // so `*` enters `.*` and `?aws` enters `.aws`; spelling them that way
        // identifies the hidden directory a credential needs.
        for directory in &entering {
            let (parent, last) = directory
                .rsplit_once('/')
                .map_or(("**", directory.as_str()), |(parent, last)| (parent, last));
            let last = if let Some(rest) = last.strip_prefix('?') {
                format!(".{rest}")
            } else if last.starts_with('*') {
                format!(".*{}", last.trim_start_matches('*'))
            } else {
                last.to_owned()
            };
            let directory = format!("{parent}/{last}");
            for glob in &selecting {
                let name = glob.rsplit('/').next().unwrap_or(glob);
                // A name matching every file reaches what its own glob does,
                // and hidden files below a public root are not checked.
                if !name.trim_matches('*').is_empty() {
                    filters.push(format!("{directory}/**/{name}"));
                }
            }
        }
    }
    (unknown || !entering.is_empty(), filters)
}

/// Whether the shell fixes the `!` that makes a glob exclude.
fn negated(glob: &Word) -> bool {
    glob.known_prefix().starts_with('!')
}

/// Whether ripgrep without a path searches standard input, which it does when
/// a pipe or a here-document feeds it; `/dev/null` is a device, so it searches
/// the directory.
fn searches_input(command: &CommandRecord) -> bool {
    matches!(
        command.stdin,
        crate::record::Stdin::Stream | crate::record::Stdin::Data(_)
    ) && !command.redirects.iter().any(|redirect| {
        redirect.fd == 0 && redirect.direction == Direction::In && redirect.target == "/dev/null"
    })
}

/// Whether `xargs` appends its items as the search's operands. macOS `xargs`
/// does not run the utility on empty input, so such a search walks the items
/// rather than its working directory; under `-I` or `-J` the items go only
/// where the marker stands.
fn xargs_appends(command: &CommandRecord) -> bool {
    command.wrappers.iter().any(|wrapper| wrapper == "xargs")
        && !command.argv[..command.program.unwrap_or(0)]
            .iter()
            .any(|word| {
                word.starts_with("-I") || word.starts_with("-J") || word.starts_with("--replace")
            })
}

fn search_arguments(
    program: &str,
    args: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) -> (Vec<Word>, Vec<Word>, bool, bool, bool) {
    let mut operands = Vec::new();
    let mut globs = Vec::new();
    let mut explicit = false;
    let mut names = false;
    let mut visibility = Visibility::default();
    let mut options = true;
    let mut position = 0;
    let mut fixed = false;
    let mut patterns = Vec::new();
    // Whether the first operand is a word whose runtime text could spell an
    // option: it is then the pattern or an option, never a file.
    let mut unproven_pattern = false;
    while position < args.len() {
        let arg = &args[position];
        if options && effects.unproven.is_none() && arg.exact() == Some("--") {
            options = false;
            position += 1;
            continue;
        }
        if options && effects.unproven.is_none() && arg.option_unknown() {
            effects.unproven = Some(position);
            unproven_pattern = operands.is_empty() && !arg.starts_with('-');
        }
        // From a word whose runtime text could spell an option that consumes
        // the next word or re-roles the operands, including the pattern
        // before it, no option consumes a word and only what widens the
        // search applies; even a later `--` may be such an option's value.
        let scan = effects.unproven.is_some();
        let mut value_option = None;
        if options && arg.starts_with("--") {
            let (key, value) = arg[2..]
                .split_once('=')
                .map_or((&arg[2..], None), |(k, v)| {
                    // The value keeps which of its bytes the shell fixes, as
                    // the `!` of an excluding glob.
                    (k, Some(arg.suffix(arg.len() - v.len())))
                });
            if key == "include" && program == "rg" {
                effects.include_advice = true;
            }
            let mut next = visibility;
            long_visibility(program, key, &mut next);
            if scan {
                visibility.hidden |= next.hidden;
                visibility.recursive |= next.recursive;
            } else {
                fixed |= key == "fixed-strings";
                names |= key == "files";
                visibility = next;
            }
            if (if program=="rg" {"regexp file glob iglob type type-not encoding replace color colors sort sortr max-depth max-filesize pre pre-glob engine threads max-columns type-add type-clear path-separator context-separator field-context-separator field-match-separator after-context before-context context max-count ignore-file dfa-size-limit regex-size-limit hyperlink-format"} else {"regexp file include exclude exclude-dir exclude-from label context after-context before-context max-count binary-files devices directories"}).split_whitespace().any(|option|option==key)
            {
                value_option = Some((key.to_owned(), value));
            }
        } else if options && arg.starts_with('-') && arg.len() > 1 {
            let (value, is_fixed, next) = short_flags(program, arg, visibility, effects);
            value_option = value.map(|(offset, ch)| short_value(program, arg, offset, ch));
            fixed |= is_fixed && !scan;
            visibility = next;
        } else {
            operands.push(arg.clone());
        }
        if let Some((key, inline)) = value_option {
            let value = inline.or_else(|| {
                position += usize::from(!scan);
                args.get(position).filter(|_| !scan).cloned()
            });
            if let Some(value) = value {
                match key.as_str() {
                    "d" | "directories" if program == "grep" && value == "recurse" => {
                        visibility.hidden = true;
                        visibility.recursive = true;
                    }
                    "e" | "regexp" if !scan => {
                        explicit = true;
                        patterns.push(value.text);
                    }
                    "f" | "file" | "ignore-file" | "exclude-from" => {
                        explicit |= !scan && matches!(key.as_str(), "f" | "file");
                        effects.targets.push(Target::from_word(
                            &value,
                            cwd,
                            host,
                            Effect::Read,
                            Walk::None,
                        ));
                    }
                    "g" | "glob" | "iglob" | "include" if !scan => globs.push(value),
                    _ => {}
                }
            }
        }
        position += 1;
    }
    if !explicit
        && !names
        && (effects.unproven.is_none() || unproven_pattern)
        && !operands.is_empty()
    {
        patterns.push(operands.remove(0).text);
    }
    effects.independent_arguments = independent_operands(args, &operands);
    effects.bre_advice = bre_advice(program, fixed, &patterns);
    let Visibility {
        hidden, recursive, ..
    } = visibility;
    (operands, globs, hidden, names, recursive)
}

/// Whether the search enters hidden names and directories, as its options
/// so far select.
#[derive(Clone, Copy, Default)]
struct Visibility {
    hidden: bool,
    no_hidden: bool,
    unrestricted: usize,
    recursive: bool,
}

fn long_visibility(program: &str, key: &str, visibility: &mut Visibility) {
    match key {
        "hidden" => {
            visibility.hidden = true;
            visibility.no_hidden = false;
        }
        "no-hidden" => {
            visibility.hidden = false;
            visibility.no_hidden = true;
        }
        "unrestricted" => {
            visibility.unrestricted += 1;
            visibility.hidden |=
                (program == "ag" || visibility.unrestricted >= 2) && !visibility.no_hidden;
        }
        "recursive" => {
            visibility.recursive = program == "grep";
            visibility.hidden |= visibility.recursive;
        }
        _ => {}
    }
}

fn short_value(program: &str, arg: &Word, offset: usize, ch: char) -> (String, Option<Word>) {
    let tail = &arg[offset + ch.len_utf8()..];
    (
        ch.to_string(),
        if tail.is_empty() {
            None
        } else {
            Some(
                arg.with_text(
                    if program == "rg" {
                        tail.trim_start_matches('=')
                    } else {
                        tail
                    }
                    .to_owned(),
                ),
            )
        },
    )
}

fn bre_advice(program: &str, fixed: bool, patterns: &[String]) -> bool {
    // Regex advice applies to patterns, never to path operands or fixed strings.
    program == "rg"
        && !fixed
        && patterns.iter().any(|pattern| {
            pattern
                .as_bytes()
                .windows(2)
                .enumerate()
                .any(|(index, bytes)| {
                    bytes == b"\\|" && (index == 0 || pattern.as_bytes()[index - 1] != b'\\')
                })
        })
}

fn short_flags(
    program: &str,
    arg: &Word,
    mut visibility: Visibility,
    effects: &mut Effects,
) -> (Option<(usize, char)>, bool, Visibility) {
    let mut fixed = false;
    for (offset, ch) in arg.char_indices().skip(1) {
        if program == "rg" {
            fixed |= ch == 'F';
            if ch == 'u' {
                visibility.unrestricted += 1;
                visibility.hidden |= visibility.unrestricted >= 2 && !visibility.no_hidden;
            }
            if ch == '.' {
                visibility.hidden = true;
            }
            if ch == 'r' {
                effects.replace_advice = true;
            }
        } else if program == "grep" && (ch == 'r' || ch == 'R') || program == "ag" && ch == 'u' {
            visibility.hidden = true;
            visibility.recursive |= program == "grep";
        }
        if (if program == "rg" {
            "efgtTEABCmMjrd"
        } else {
            "efABCmdD"
        })
        .contains(ch)
        {
            return (Some((offset, ch)), fixed, visibility);
        }
    }
    (None, fixed, visibility)
}
