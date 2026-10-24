use crate::{CoverageGap, shell::CommandRecord};

pub use crate::record::Target;
use crate::record::{Direction, Effect, HostFacts, Role, Via, Walk, Word};
mod clients;
mod resource_changes;
mod secrets;
const READERS: &str = "cat head tail less more bat sed awk jq yq base64 xxd od strings diff openssl plutil cp tee tar source . sort uniq cut nl fold rev paste comm join iconv hexdump hd zcat gzcat bzcat xzcat ag ack tac column pr vim vi nvim view perl ruby dd scp rsync zip ed ex hg svn sh bash zsh dash ksh wget php zgrep zless zmore";
const DATA_PROGRAMS: &str =
    "echo printf print : true false export set unset typeset declare local kill";

#[derive(Debug, Default)]
pub struct Effects {
    #[cfg(test)]
    owner_visits: usize,
    #[cfg(test)]
    argv_words: usize,
    pub(crate) independent_arguments: bool,
    /// The first argument whose role the program's grammar cannot prove,
    /// because runtime text there could spell an option that consumes or
    /// re-roles the words after it.
    pub(crate) unproven: Option<usize>,
    pub targets: Vec<Target>,
    pub gaps: Vec<CoverageGap>,
    pub code: Vec<String>,
    pub inline: Vec<String>,
    pub dump: bool,
    pub variable: bool,
    pub token: bool,
    pub keychain: bool,
    pub stored_secret: bool,
    pub trace: bool,
    pub hidden_content: bool,
    pub replace_advice: bool,
    pub include_advice: bool,
    pub bre_advice: bool,
    pub hidden_listing: bool,
    pub consumes_listing: bool,
}

/// Marks the words after `--` as operands, unless an earlier word could
/// spell an option that consumes the `--`.
fn label_options(args: &[Word]) -> std::borrow::Cow<'_, [Word]> {
    let mut labelled = std::borrow::Cow::Borrowed(args);
    for (index, word) in args.iter().enumerate() {
        if word.option_unknown() {
            break;
        }
        if word.exact() == Some("--") {
            for operand in &mut labelled.to_mut()[index + 1..] {
                operand.role = Role::Path;
            }
            break;
        }
    }
    labelled
}

/// The options a getopt_long parser is known to accept: flags, and options
/// that take a value glued as `-xVALUE` or `--name=VALUE`, or as the next
/// word. Any other option may take the next word as its value.
struct Getopt {
    short_flags: &'static str,
    short_values: &'static str,
    flags: &'static [&'static str],
    values: &'static [&'static str],
}

/// The words holding the values of the `wanted` options, spelled `-x` or
/// `--name`, and the first word whose role the parser could take
/// differently: one the shell may split, runtime text that could spell an
/// option, or an option the grammar does not know. A known `--` ends the
/// options with every role proven.
fn option_values(args: &[Word], getopt: &Getopt, wanted: &[&str]) -> (Vec<usize>, Option<usize>) {
    let mut values = Vec::new();
    let mut index = 0;
    while let Some(word) = args.get(index) {
        let at = index;
        index += 1;
        let prefix = word.known_prefix();
        if !word.option_unknown() && (prefix == "-" || !prefix.starts_with('-')) {
            continue;
        }
        // Of runtime text, only a known option name with a glued value is
        // sure to be that option.
        let (text, glued_only) = match word.exact() {
            Some(text) => (text, false),
            None if word.fixed_position => (prefix, true),
            None => return (values, Some(at)),
        };
        if text == "--" && !glued_only {
            return (values, None);
        }
        let (key, holder) = if let Some(long) = text.strip_prefix("--") {
            let (name, glued) = long
                .split_once('=')
                .map_or((long, false), |(name, _)| (name, true));
            let key = &text[..name.len() + 2];
            if glued_only && !glued {
                return (values, Some(at));
            }
            if getopt.flags.contains(&key) {
                continue;
            }
            if !getopt.values.contains(&key) {
                return (values, Some(at));
            }
            (key, if glued { at } else { index })
        } else {
            if glued_only {
                return (values, Some(at));
            }
            // A short option's key is its letter.
            let Some((offset, letter)) = text
                .char_indices()
                .skip(1)
                .find(|(_, letter)| !getopt.short_flags.contains(*letter))
            else {
                continue;
            };
            if !getopt.short_values.contains(letter) {
                return (values, Some(at));
            }
            let end = offset + letter.len_utf8();
            (
                &text[offset..end],
                if end < text.len() { at } else { index },
            )
        };
        if holder == index {
            if !args.get(index).is_some_and(|value| value.fixed_position) {
                return (values, Some(index));
            }
            index += 1;
        }
        if wanted
            .iter()
            .any(|option| *option == key || option.strip_prefix('-') == Some(key))
        {
            values.push(holder);
        }
    }
    (values, None)
}

fn modelled_program(program: &str) -> bool {
    // A partial option adapter does not by itself model the whole command.
    READERS.split_whitespace().chain(DATA_PROGRAMS.split_whitespace()).chain(
        "stat test [ chmod chown chgrp chflags touch rm rmdir mkdir mv ln wc file shasum sha1sum sha256sum md5 md5sum cksum realpath readlink basename dirname cd pushd popd gh ls tree du install sftp curl git docker node bun deno kubectl ssh ssh-add ssh-keygen dotenvx npm pnpm yarn rg grep find fd".split_whitespace()
    ).any(|name| name == program)
}

fn printenv_signature(code: &str) -> bool {
    let boundary = |c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-';
    code.match_indices("printenv").any(|(start, name)| {
        code[..start].chars().next_back().is_none_or(boundary)
            && code[start + name.len()..]
                .chars()
                .next()
                .is_none_or(boundary)
    })
}

fn secret_name(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    ["TOKEN", "SECRET", "KEY", "PASSWORD", "CREDENTIAL"]
        .iter()
        .any(|part| name.contains(part))
}

pub(crate) fn shows_hidden(args: &[Word]) -> bool {
    args.iter().any(|w| {
        ["--hidden", "--unrestricted"].contains(&w.as_str())
            || w.starts_with('-')
                && !w.starts_with("--")
                && w[1..].chars().all(|c| c.is_ascii_alphabetic())
                && w.contains(['H', 'u'])
    })
}

pub(crate) fn list_file_sources(command: &CommandRecord) -> Vec<&crate::record::StreamOutput> {
    let name = command
        .program
        .and_then(|index| command.argv.get(index))
        .map_or("", |word| word.rsplit('/').next().unwrap_or(word));
    let xargs = command.wrappers.iter().any(|wrapper| wrapper == "xargs");
    command
        .argv
        .iter()
        .enumerate()
        .filter_map(|(index, word)| {
            let stream = word.stream.as_deref()?;
            let option = word.split_once('=').map_or_else(
                || {
                    command
                        .argv
                        .get(index.wrapping_sub(1))
                        .map_or("", Word::as_str)
                },
                |(option, _)| option,
            );
            ((xargs && ["-a", "--arg-file"].contains(&option))
                || (name == "tar" && option == "-T")
                || (["tar", "rsync"].contains(&name) && option == "--files-from")
                || (["sort", "du"].contains(&name) && option == "--files0-from"))
                .then_some(stream)
        })
        .collect()
}

fn operand_value(word: &Word) -> Option<&str> {
    let mut value = word.value.as_str();
    if value.starts_with('-') {
        value = value.split_once('=')?.1;
    }
    if let Some(path) = value.strip_prefix('@') {
        return (!path.is_empty()).then_some(path);
    }
    if let Some(at) = value.find('@') {
        let prefix = &value[..at];
        if (prefix.ends_with('=') || prefix.ends_with(':'))
            && prefix
                .trim_end_matches(['=', ':'])
                .chars()
                .all(|c| !matches!(c, '=' | '@') && !space(c))
            && !prefix.trim_end_matches(['=', ':']).is_empty()
        {
            value = &value[at + 1..];
        }
    }
    (!value.is_empty()).then_some(value)
}

fn independent_operands(args: &[Word], operands: &[Word]) -> bool {
    let count = args.iter().filter(|word| word.cardinality_unknown).count();
    count != 0
        && count
            == operands
                .iter()
                .filter(|word| word.cardinality_unknown)
                .count()
        && operands.iter().all(|word| {
            !word.cardinality_unknown || !word.starts_with('-') || word.role == Role::Path
        })
}

/// A read of the files below `root` that the search glob `glob` selects.
fn filter_target(
    root: &Word,
    glob: &str,
    via: Via,
    reaches_hidden: bool,
    cwd: &str,
    host: HostFacts<'_>,
) -> Target {
    let mut target = Target::new(
        crate::filesystem::absolute_input(
            &crate::filesystem::search_glob(
                &if root.globs || root.shell_matches {
                    root.text.clone()
                } else {
                    crate::filesystem::literal_glob_root(&root.text)
                },
                glob,
            ),
            cwd,
            host.home,
        ),
        Effect::Read,
        Walk::None,
        via,
    );
    target.glob = true;
    target.glob_hidden = reaches_hidden;
    target
}

/// A hidden walk of `root` that reads only the files whose names match one
/// of `names`, which it may find in any hidden directory.
fn narrowed_walk(root: &Word, names: &[String], cwd: &str, host: HostFacts<'_>) -> Target {
    let mut walk = Target::from_word(root, cwd, host, Effect::Read, Walk::Hidden);
    walk.narrowed = Some(narrowed_reads(root, names, cwd, host));
    walk.bounds = Some(names.to_vec());
    walk
}

fn narrowed_reads(root: &Word, names: &[String], cwd: &str, host: HostFacts<'_>) -> Vec<Target> {
    names
        .iter()
        .map(|name| filter_target(root, name, Via::Narrow, true, cwd, host))
        .collect()
}

fn space(c: char) -> bool {
    c.is_whitespace() && c != '\u{85}' || c == '\u{feff}'
}

mod archive;
mod code;
mod git;
mod inference;
mod interpreters;
mod listing;
mod search;
mod shells;
#[cfg(test)]
mod tests;
mod wrappers;

use archive::infer_tar;
pub use code::code_paths;
use git::infer_git;
pub use inference::infer;
use inference::infer_at;
use interpreters::{interpreter_code, perl_exec_argv};
use listing::{find_printed_walks, infer_listing};
use search::infer_search;
pub(crate) use shells::{ShellSource, shell_invocation};
use wrappers::{at, child, infer_forwarded, xargs_content_consumer};

mod programs;
