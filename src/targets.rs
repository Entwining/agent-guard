use crate::{CoverageGap, shell::CommandRecord};

pub use crate::record::Target;
use crate::record::{Direction, Effect, HostFacts, OptionRole, Role, Via, Walk, Word};
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

fn label_options<'a>(
    program: &str,
    args: &'a [Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) -> std::borrow::Cow<'a, [Word]> {
    let mut labelled = std::borrow::Cow::Borrowed(args);
    let mut options = true;
    // Program options take precedence over generic name-only options;
    // keep values in place for the adapter that owns their role.
    for (index, word) in args.iter().enumerate() {
        if word == "--" {
            options = false;
            continue;
        }
        if !options {
            labelled.to_mut()[index].role = Role::Path;
            continue;
        }
        let key = if word.value.starts_with('-') {
            word.value.split_once('=').map_or("", |(key, _)| key)
        } else {
            args.get(index.wrapping_sub(1)).map_or("", Word::as_str)
        };
        if clients::option(program, key).is_none()
            && ["--exclude", "--exclude-dir", "--include"].contains(&key)
        {
            let option_value = if word.value.starts_with('-') {
                word.value
                    .split_once('=')
                    .map_or(word.value.as_str(), |(_, value)| value)
            } else {
                &word.value
            };
            labelled.to_mut()[index].role = Role::Option(OptionRole::Name);
            let mut target = Target::from_word(
                &word.with_text(option_value.into()),
                cwd,
                host,
                Effect::Name,
                Walk::None,
            );
            target.via = Via::Option;
            effects.targets.push(target);
        }
    }
    labelled
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
#[cfg(test)]
mod tests;
mod wrappers;

use archive::infer_tar;
pub use code::code_paths;
use git::infer_git;
pub use inference::infer;
use inference::infer_at;
use interpreters::{interpreter_code, perl_exec_argv};
use listing::infer_listing;
use search::infer_search;
use wrappers::{at, child, infer_wrapper, xargs_content_consumer};

mod programs;
