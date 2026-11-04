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
    /// The program writes a credential variable's value to its output.
    pub variable: bool,
    /// The program writes that value only to its standard output.
    pub variable_stdout_only: bool,
    /// The program takes its standard input as a credential it uses without
    /// writing it to its output.
    pub credential_stdin: bool,
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

/// How the generic operand role reads the words a program's owner leaves
/// to it.
pub(super) struct Operands {
    pub walk: Walk,
    /// The words the owner gave a role of its own.
    pub claimed: Vec<usize>,
    /// Whether the program takes `@FILE` literally; one whose conventions
    /// are unknown may read either name.
    pub literal: bool,
}

/// Adds a target for each option value `binding` gives a role other than
/// data, a read keeping the program's operand `walk`, and returns the words
/// it claims from the generic operand role: the options and their values.
fn option_targets(
    args: &[Word],
    binding: crate::options::Binding,
    walk: Walk,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) -> Vec<usize> {
    for bound in &binding.values {
        let effect = bound.value.effect;
        if effect == Effect::Name {
            continue;
        }
        let mut target = Target::from_word(
            &bound.value.word(args),
            cwd,
            host,
            effect,
            if effect == Effect::Read {
                walk
            } else {
                Walk::None
            },
        );
        target.via = Via::Option;
        effects.targets.push(target);
    }
    if let Some(start) = binding.unproven {
        effects.unproven = Some(effects.unproven.map_or(start, |known| known.min(start)));
    }
    binding.options
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

/// Environment names carry an uppercase name or prefix, as in
/// `TF_VAR_db_password`; a name without an uppercase letter is the script's
/// own variable, such as a loop's `key`.
fn secret_variable(name: &str) -> bool {
    name.contains(|c: char| c.is_ascii_uppercase()) && secret_name(name)
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

/// The names a program may read from `word`: its text, or unless the word is
/// `literal`, the value glued after `=` to an option and the file an `@FILE`
/// convention names, which only some programs define.
fn operand_values(word: &Word, literal: bool) -> impl Iterator<Item = &str> {
    let mut value = word.value.as_str();
    if !literal && value.starts_with('-') {
        value = value.split_once('=').map_or("", |(_, value)| value);
    }
    let file = if literal {
        None
    } else {
        value.strip_prefix('@').or_else(|| {
            let at = value.find('@')?;
            let key = &value[..at];
            let name = key.trim_end_matches(['=', ':']);
            (name.len() < key.len()
                && !name.is_empty()
                && name.chars().all(|c| !matches!(c, '=' | '@') && !space(c)))
            .then(|| &value[at + 1..])
        })
    };
    [Some(value), file]
        .into_iter()
        .flatten()
        .filter(|name| !name.is_empty())
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
