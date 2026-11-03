use super::{Effects, operand_values, space};
use crate::CoverageGap;
use crate::options::{Grammar, Syntax};
use crate::record::Effect::{Name, Read, Use, Write};
use crate::record::{Effect, HostFacts, Role, Target, Via, Walk, Word};

// GNU cp; macOS cp has no value options, so a letter that is a macOS flag
// takes a value only when glued.
const CP: Grammar = Grammar {
    values: &[
        ("-t --target-directory", Write),
        ("--suffix --sparse --no-preserve", Name),
    ],
    attached: &[("-S --backup --reflink --preserve --context --update", Name)],
    stops: true,
    ..Grammar::partial(Syntax::Getopt, &[])
};

// macOS and GNU install. A letter whose arity differs between the two takes a
// value only when glued.
const INSTALL: Grammar = Grammar {
    values: &[
        ("-t --target-directory -M", Write),
        ("-N", Read),
        ("--strip-program", Use),
        ("-B -f -g --group -h -l -m --mode -o --owner --suffix", Name),
    ],
    attached: &[("-D -S -T", Name)],
    stops: true,
    ..Grammar::partial(Syntax::Getopt, &[])
};

const DOTENVX: Grammar = Grammar::partial(Syntax::Getopt, &[("-f --file --env-file", Use)]);
const SSH_KEYGEN: Grammar = Grammar::partial(Syntax::Getopt, &[("-f", Use)]);
const KUBECTL: Grammar = Grammar::partial(Syntax::Clap, &[("--kubeconfig", Use)]);
const NPM: Grammar = Grammar::partial(Syntax::Getopt, &[("--userconfig", Use)]);
const DOCKER: Grammar = Grammar::partial(Syntax::Clap, &[("--env-file", Use)]);

// The options of openrsync and rsync that agents use, from their manuals.
const RSYNC: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "0468ACDEFHIJKLNOPRSUVWXabcdghiklmnopqrstuvxyz",
    flags: "--archive --recursive --verbose --quiet --checksum --relative --dirs --links --copy-links --copy-unsafe-links --safe-links --keep-dirlinks --copy-dirlinks --hard-links --perms --executability --acls --xattrs --extended-attributes --owner --group --devices --specials --times --atimes --crtimes --omit-dir-times --omit-link-times --super --fake-super --sparse --preallocate --dry-run --whole-file --one-file-system --existing --ignore-existing --ignore-non-existing --remove-source-files --del --delete --delete-before --delete-during --delete-delay --delete-after --delete-excluded --ignore-missing-args --delete-missing-args --ignore-errors --force --partial --progress --prune-empty-dirs --numeric-ids --size-only --ignore-times --update --inplace --append --append-verify --compress --cvs-exclude --from0 --protect-args --secluded-args --stats --human-readable --itemize-changes --list-only --blocking-io --no-motd --mkpath --backup --delay-updates --fuzzy --no-implied-dirs --no-perms --no-times --no-owner --no-group --no-whole-file --no-compress --no-inc-recursive --cache --no-cache --8-bit-output --ipv4 --ipv6 --help --version",
    values: &[
        (
            "--files-from --exclude-from --include-from --read-batch --early-input --config -e --rsh -T --temp-dir --backup-dir --partial-dir --compare-dest --copy-dest --link-dest --confine-root",
            Read,
        ),
        ("--log-file --write-batch --only-write-batch", Write),
        ("--password-file", Use),
        (
            "-f --filter --exclude --include --out-format --log-format --log-file-format --info --debug --stderr --suffix --chmod --checksum-choice --cc -B --block-size --rsync-path --max-delete --max-size --min-size --max-alloc --usermap --groupmap --chown --timeout --contimeout -@ --modify-window --compress-choice --zc --compress-level --zl --compress-threads --skip-compress --copy-as --address --port --sockopts --outbuf -M --remote-option --bwlimit --stop-after --stop-at --protocol --iconv --checksum-seed --dparam",
            Name,
        ),
    ],
    attached: &[],
    stops: false,
    complete: true,
};

struct Spec {
    operand: Effect,
    walk: Walk,
    options: Option<&'static Grammar>,
    remote: bool,
    destination: Option<Effect>,
}

/// The client roles of `program`, or None when the guard does not model the
/// program or the command form its words select.
fn spec(program: &str, words: &[Word]) -> Option<Spec> {
    let mut spec = Spec {
        operand: Effect::Read,
        walk: Walk::Visible,
        options: None,
        remote: false,
        destination: None,
    };
    match program {
        "cp" => {
            spec.destination = Some(Effect::Write);
            spec.options = Some(&CP);
            spec.walk = Walk::None;
        }
        "install" => {
            spec.destination = Some(Effect::Write);
            spec.options = Some(&INSTALL);
        }
        "ssh" => spec.operand = Effect::Name,
        "scp" => spec.remote = true,
        // Client operand and option roles are not exceptions to protection.
        "rsync" => {
            spec.remote = true;
            spec.options = Some(&RSYNC);
        }
        "tee" => spec.operand = Effect::Write,
        "ssh-add" => spec.operand = Effect::Use,
        "dotenvx" => spec.options = Some(&DOTENVX),
        "dd" => spec.walk = Walk::None,
        "ssh-keygen" => spec.options = Some(&SSH_KEYGEN),
        "kubectl" => spec.options = Some(&KUBECTL),
        "npm" => spec.options = Some(&NPM),
        "curl" => spec.operand = Effect::Name,
        "wget" => {
            spec.operand = Effect::Name;
            spec.options = Some(&WGET);
        }
        "docker" => {
            spec.operand = Effect::Name;
            spec.options = Some(&DOCKER);
        }
        "sftp" | "ctags" => {}
        "cargo" => return cargo::spec(words),
        "diskutil" | "tmutil" | "dscl" => return query_verbs::spec(program, words),
        _ => return None,
    }
    Some(spec)
}

/// The role of operands that come from a process item list.
pub(super) fn operand(program: &str, words: &[Word]) -> Option<Effect> {
    spec(program, words).map(|spec| spec.operand)
}

struct Context<'a> {
    words: &'a [Word],
    cwd: &'a str,
    host: HostFacts<'a>,
    spec: Spec,
    claimed: Vec<bool>,
    targets: Vec<Target>,
    /// The first word whose role the adapter's grammar cannot prove.
    unproven: Option<usize>,
}

impl<'a> Context<'a> {
    fn text(&self, index: usize) -> &'a str {
        self.words.get(index).map_or("", Word::as_str)
    }
    fn add(
        &mut self,
        path: &str,
        index: Option<usize>,
        effect: Effect,
        quoted: Option<bool>,
    ) -> &mut Target {
        let mut word = self
            .words
            .get(index.unwrap_or(usize::MAX))
            .cloned()
            .unwrap_or_else(|| Word::literal(String::new()));
        word.text = path.into();
        if let Some(quoted) = quoted {
            word.raw = if quoted { "\"" } else { "" }.into();
        }
        let mut target = Target::from_word(&word, self.cwd, self.host, effect, self.spec.walk);
        target.via = Via::Option;
        if let Some(index) = index {
            self.claimed[index] = true;
        }
        let at = self.targets.len();
        self.targets.push(target);
        &mut self.targets[at]
    }
    /// Adds the targets of the option values the spec's grammar models, and
    /// returns whether an option names the destination directory.
    fn options(&mut self) -> bool {
        let Some(grammar) = self.spec.options else {
            return false;
        };
        let binding = crate::options::bind(self.words, grammar);
        let sends = self.sends();
        for bound in &binding.values {
            if bound.value.effect != Effect::Name {
                let mut target = Target::from_word(
                    &bound.value.word(self.words),
                    self.cwd,
                    self.host,
                    bound.value.effect,
                    self.spec.walk,
                );
                target.via = Via::Option;
                target.sends = sends;
                self.targets.push(target);
            }
        }
        for &index in &binding.options {
            self.claimed[index] = true;
        }
        self.unproven = binding.unproven;
        binding
            .values
            .iter()
            .any(|bound| ["-t", "--target-directory"].contains(&bound.name))
    }
    fn sends(&self) -> bool {
        self.spec.remote && self.words.iter().any(|word| remote(&word.value))
    }
    /// Adds the targets of the words no adapter claimed, and returns the
    /// first word whose role it cannot prove.
    fn fallback(&mut self, into: bool) -> Option<usize> {
        // Runtime text that could spell an option makes later roles, and
        // which operand comes last, provisional.
        let uncertain: Vec<usize> = (0..self.words.len())
            .filter(|&index| {
                !self.claimed[index]
                    && self.words[index].role != Role::Path
                    && self.words[index].option_unknown()
            })
            .chain(self.unproven)
            .collect();
        let unproven = uncertain.iter().copied().min();
        let last = (0..self.words.len())
            .rfind(|&index| !self.claimed[index] && !self.words[index].starts_with('-'));
        let sends = self.sends();
        for (index, word) in self.words.iter().enumerate() {
            if self.claimed[index] {
                continue;
            }
            for value in operand_values(word, word.role == Role::Path) {
                let mut effect = self.spec.operand;
                if (self.spec.remote || self.spec.destination.is_some() && !into)
                    && Some(index) == last
                    && !word.globs
                {
                    effect = self.spec.destination.unwrap_or(Effect::Write);
                    // Only another uncertain word can make this one a source.
                    if uncertain.iter().any(|&other| other != index) {
                        let mut target = Target::from_word(
                            &word.with_text(value.into()),
                            self.cwd,
                            self.host,
                            Effect::Read,
                            self.spec.walk,
                        );
                        target.sends = sends;
                        self.targets.push(target);
                    }
                }
                if self.spec.remote && remote(&word.value) {
                    effect = Effect::Name;
                }
                let mut target = Target::from_word(
                    &word.with_text(value.into()),
                    self.cwd,
                    self.host,
                    effect,
                    self.spec.walk,
                );
                target.sends = sends;
                self.targets.push(target);
            }
        }
        unproven
    }
}

fn remote(value: &str) -> bool {
    value.starts_with("rsync://")
        || value.split_once(':').is_some_and(|(host, _)| {
            let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
            !host.is_empty() && !host.contains(['/', '@', ':'])
        })
}

/// Adds the targets of a modelled client; false leaves `program` unmodelled.
pub(super) fn infer(
    program: &str,
    words: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) -> bool {
    let Some(spec) = spec(program, words) else {
        return false;
    };
    let mut context = Context {
        words,
        cwd,
        host,
        spec,
        claimed: vec![false; words.len()],
        targets: Vec::new(),
        unproven: None,
    };
    match program {
        "cp" => {
            if words.iter().any(|word| {
                word == "--recursive"
                    || word.strip_prefix('-').is_some_and(|flags| {
                        flags
                            .chars()
                            .take_while(|c| *c != '-')
                            .any(|c| c == 'r' || c == 'R')
                    })
            }) {
                context.spec.walk = Walk::Visible;
            }
        }
        "ssh" | "scp" | "sftp" => ssh(program, &mut context),
        "curl" => curl(&mut context),
        "wget" => wget(&mut context),
        "docker" => docker(&mut context),
        "ctags" => ctags(&mut context),
        "cargo" => cargo(&mut context, effects),
        "dd" => {
            for (index, word) in words.iter().enumerate() {
                context.claimed[index] = true;
                if let Some(path) = word.strip_prefix("if=") {
                    context.add(path, Some(index), Effect::Read, None);
                } else if let Some(path) = word.strip_prefix("of=") {
                    context.add(path, Some(index), Effect::Write, None);
                }
            }
        }
        _ => {}
    }
    let into = context.options();
    effects.unproven = context.fallback(into);
    effects.targets.extend(context.targets);
    true
}

fn ctags(context: &mut Context<'_>) {
    for (index, word) in context.words.iter().enumerate() {
        if word.role == Role::Path {
            continue;
        }
        let value = word.strip_prefix("--exclude=").or_else(|| {
            (context.text(index.wrapping_sub(1)) == "--exclude").then_some(word.as_str())
        });
        // ctags interprets this pattern value as a file of patterns.
        if let Some(path) = value.and_then(|value| value.strip_prefix('@'))
            && !path.is_empty()
        {
            context.add(path, Some(index), Effect::Read, None);
        }
        // ctags documents long option values only after `=`, where any
        // pattern but `@FILE` names files to skip.
        if word
            .known_prefix()
            .strip_prefix("--exclude=")
            .is_some_and(|pattern| !pattern.is_empty() && !pattern.starts_with('@'))
        {
            context.claimed[index] = true;
        }
    }
}

fn short_value(text: &str, wanted: char) -> Option<&str> {
    text.strip_prefix('-')?
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_alphabetic())
        .find(|(_, c)| *c == wanted)
        .map(|(at, c)| &text[at + 1 + c.len_utf8()..])
}

mod cargo;
mod curl;
mod docker;
mod query_verbs;
mod ssh;
mod wget;
use cargo::cargo;
pub(super) use curl::CURL_VALUE_LETTERS;
use curl::curl;
use docker::docker;
use ssh::ssh;
use wget::{WGET, wget};
