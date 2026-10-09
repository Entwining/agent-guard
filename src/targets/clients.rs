use super::{operand_value, space};
use crate::record::{Effect, HostFacts, OptionRole, Role, Target, Via, Walk, Word};

struct Spec {
    operand: Effect,
    walk: Walk,
    options: &'static [(&'static str, Effect)],
    remote: bool,
    destination: Option<Effect>,
}

fn spec(program: &str) -> Spec {
    let mut spec = Spec {
        operand: Effect::Read,
        walk: Walk::Visible,
        options: &[],
        remote: false,
        destination: None,
    };
    match program {
        "cp" | "install" => {
            spec.destination = Some(Effect::Write);
            spec.options = &[("-t", Effect::Write), ("--target-directory", Effect::Write)];
            if program == "cp" {
                spec.walk = Walk::None;
            }
        }
        "ssh" => spec.operand = Effect::Name,
        "scp" => spec.remote = true,
        // Client operand and option roles are not exceptions to protection.
        "rsync" => {
            spec.remote = true;
            spec.options = &[
                ("--files-from", Effect::Read),
                ("--exclude-from", Effect::Read),
                ("--include-from", Effect::Read),
            ];
        }
        "tee" => spec.operand = Effect::Write,
        "ssh-add" => spec.operand = Effect::Use,
        "dotenvx" => {
            spec.options = &[
                ("-f", Effect::Use),
                ("--file", Effect::Use),
                ("--env-file", Effect::Use),
            ]
        }
        "dd" => spec.walk = Walk::None,
        "ssh-keygen" => spec.options = &[("-f", Effect::Use)],
        "kubectl" => spec.options = &[("--kubeconfig", Effect::Use)],
        "npm" => spec.options = &[("--userconfig", Effect::Use)],
        "curl" => {
            spec.operand = Effect::Name;
            spec.options = CURL_USE;
        }
        "wget" => {
            spec.operand = Effect::Name;
            spec.options = WGET_USE;
        }
        "docker" => {
            spec.operand = Effect::Name;
            spec.options = &[("--env-file", Effect::Use)];
        }
        _ => {}
    }
    spec
}

pub(super) fn option(program: &str, key: &str) -> Option<Effect> {
    spec(program)
        .options
        .iter()
        .find_map(|(name, effect)| (*name == key).then_some(*effect))
}

struct Context<'a> {
    words: &'a [Word],
    cwd: &'a str,
    host: HostFacts<'a>,
    spec: Spec,
    claimed: Vec<bool>,
    targets: Vec<Target>,
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
    fn option_effect(&self, index: usize) -> Option<Effect> {
        if self.words[index].role == Role::Path {
            return None;
        }
        let value = &self.words[index].value;
        let previous = self.text(index.wrapping_sub(1));
        let key = if value.starts_with('-') {
            value.split_once('=').map_or("", |(key, _)| key)
        } else if previous.starts_with('-') && !previous.starts_with("--") {
            previous
                .get(previous.len().saturating_sub(1)..)
                .unwrap_or("")
        } else {
            previous
        };
        self.spec.options.iter().find_map(|(option, effect)| {
            (*option == key || key.len() == 1 && option.strip_prefix('-') == Some(key))
                .then_some(*effect)
        })
    }
    fn fallback(&mut self) {
        let into = self.spec.destination.is_some()
            && self.words.iter().any(|word| {
                word.role != Role::Path
                    && (word.starts_with("--t")
                        && "--target-directory".starts_with(
                            word.split_once('=').map_or(word.as_str(), |(key, _)| key),
                        )
                        || word.starts_with('-') && !word.starts_with("--") && word.contains('t'))
            });
        let last = self
            .words
            .iter()
            .enumerate()
            .filter(|(i, word)| !word.starts_with('-') && self.option_effect(*i).is_none())
            .map(|(i, _)| i)
            .next_back();
        let sends = self.spec.remote && self.words.iter().any(|word| remote(&word.value));
        for (index, word) in self.words.iter().enumerate() {
            if self.claimed[index] || word.role == Role::Option(OptionRole::Name) {
                continue;
            }
            if word.role != Role::Path
                && let Some(flags) = word.strip_prefix('-').filter(|s| !s.starts_with('-'))
                && let Some((at, (_, effect))) = flags.char_indices().find_map(|(at, letter)| {
                    self.spec
                        .options
                        .iter()
                        .find(|(option, _)| option.strip_prefix('-') == Some(&letter.to_string()))
                        .map(|option| (at, option))
                })
            {
                let value = &flags[at + 1..];
                if !value.is_empty() {
                    let mut target = Target::from_word(
                        &word.with_text(value.into()),
                        self.cwd,
                        self.host,
                        *effect,
                        self.spec.walk,
                    );
                    target.via = Via::Option;
                    target.sends = sends;
                    self.targets.push(target);
                    continue;
                }
            }
            let value = if word.role == Role::Path {
                (!word.value.is_empty()).then_some(word.value.as_str())
            } else {
                operand_value(word)
            };
            let Some(value) = value else {
                continue;
            };
            let mut effect = self.option_effect(index).unwrap_or(self.spec.operand);
            if (self.spec.remote || self.spec.destination.is_some() && !into)
                && Some(index) == last
                && !word.globs
            {
                effect = self.spec.destination.unwrap_or(Effect::Write);
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
}

fn remote(value: &str) -> bool {
    value.starts_with("rsync://")
        || value.split_once(':').is_some_and(|(host, _)| {
            let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
            !host.is_empty() && !host.contains(['/', '@', ':'])
        })
}

pub(super) fn infer(program: &str, words: &[Word], cwd: &str, host: HostFacts<'_>) -> Vec<Target> {
    let mut context = Context {
        words,
        cwd,
        host,
        spec: spec(program),
        claimed: vec![false; words.len()],
        targets: Vec::new(),
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
    context.fallback();
    context.targets
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
    }
}

fn short_value(text: &str, wanted: char) -> Option<&str> {
    text.strip_prefix('-')?
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_alphabetic())
        .find(|(_, c)| *c == wanted)
        .map(|(at, c)| &text[at + 1 + c.len_utf8()..])
}

mod curl;
mod docker;
mod ssh;
mod wget;
pub(super) use curl::CURL_VALUE_LETTERS;
use curl::{CURL_USE, curl};
use docker::docker;
use ssh::ssh;
use wget::{WGET_USE, wget};
