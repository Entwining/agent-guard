use crate::record::{Effect, HostFacts, Target, Via, Walk, Word};

struct Spec {
    operand: Effect,
    walk: Walk,
    options: &'static [(&'static str, Effect)],
    remote: bool,
}

fn spec(program: &str) -> Spec {
    let mut spec = Spec {
        operand: Effect::Read,
        walk: Walk::Visible,
        options: &[],
        remote: false,
    };
    match program {
        "ssh" => spec.operand = Effect::Name,
        "scp" => spec.remote = true,
        "dd" => spec.walk = Walk::None,
        "ssh-keygen" => spec.options = &[("-f", Effect::Use)],
        "kubectl" => spec.options = &[("--kubeconfig", Effect::Use)],
        "npm" => spec.options = &[("--userconfig", Effect::Use)],
        _ => {}
    }
    spec
}

struct Context<'a> {
    words: &'a [Word],
    cwd: &'a str,
    host: HostFacts<'a>,
    spec: Spec,
    claimed: Vec<bool>,
    targets: Vec<Target>,
}

impl Context<'_> {
    fn text(&self, index: usize) -> &str {
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
        let last = self
            .words
            .iter()
            .enumerate()
            .filter(|(i, word)| !word.starts_with('-') && self.option_effect(*i).is_none())
            .map(|(i, _)| i)
            .next_back();
        let sends = self.spec.remote && self.words.iter().any(|word| remote(&word.value));
        for (index, word) in self.words.iter().enumerate() {
            if self.claimed[index] {
                continue;
            }
            if let Some(flags) = word.strip_prefix('-').filter(|s| !s.starts_with('-'))
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
            let Some(value) = operand_value(word) else {
                continue;
            };
            let mut effect = self.option_effect(index).unwrap_or(self.spec.operand);
            if self.spec.remote && Some(index) == last && !word.globs {
                effect = Effect::Write;
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

pub(super) fn operand_value(word: &Word) -> Option<&str> {
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

fn space(c: char) -> bool {
    c.is_whitespace() && c != '\u{85}' || c == '\u{feff}'
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
        "ssh" | "scp" | "sftp" => ssh(program, &mut context),
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

fn ssh(program: &str, context: &mut Context<'_>) {
    let letters = match program {
        "ssh" => "BDEFIJLOPQRSWbceilmopw",
        "scp" => "DFJPSXcilo",
        _ => "BDFJPRSXbcilos",
    };
    let mut index = 0;
    let mut operands = 0;
    while let Some(word) = context.words.get(index) {
        if word == "--" {
            break;
        }
        if !word.starts_with('-') || word.len() < 2 {
            operands += 1;
            if operands == if program == "ssh" { 2 } else { 1 } {
                break;
            }
            index += 1;
            continue;
        }
        let Some((at, letter)) = word
            .char_indices()
            .skip(1)
            .find(|(_, c)| letters.contains(*c))
        else {
            index += 1;
            continue;
        };
        let glued = &word[at + letter.len_utf8()..];
        let value = if glued.is_empty() {
            index += 1;
            context.text(index)
        } else {
            glued
        };
        if index >= context.words.len() {
            break;
        }
        let mut effect = match letter {
            'i' | 'F' | 'S' => Some(Effect::Use),
            'E' => Some(Effect::Write),
            'b' if program == "sftp" && value != "-" => Some(Effect::Read),
            _ => None,
        };
        let mut paths = vec![value.to_owned()];
        if letter == 'o' {
            paths.clear();
            effect = None;
            let name_end = value
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(value.len());
            let rest = &value[name_end..];
            let rest = if let Some(rest) = rest.trim_start_matches(space).strip_prefix('=') {
                Some(rest.trim_start_matches(space))
            } else if rest.chars().next().is_some_and(space) {
                Some(rest.trim_start_matches(space))
            } else {
                None
            };
            if let Some(rest) = rest.filter(|rest| !rest.is_empty()) {
                effect = match value[..name_end].to_ascii_lowercase().as_str() {
                    "identityfile"
                    | "certificatefile"
                    | "globalknownhostsfile"
                    | "revokedhostkeys"
                    | "pkcs11provider" => Some(Effect::Use),
                    "userknownhostsfile" => Some(Effect::Write),
                    _ => None,
                };
                let mut remaining = rest;
                while !remaining.is_empty() {
                    remaining = remaining.trim_start_matches(space);
                    if remaining.is_empty() {
                        break;
                    }
                    let end = if let Some(quoted) = remaining.strip_prefix('"') {
                        quoted.find('"').map_or_else(
                            || remaining.find(space).unwrap_or(remaining.len()),
                            |at| at + 2,
                        )
                    } else {
                        remaining.find(space).unwrap_or(remaining.len())
                    };
                    let mut path = remaining[..end].replace('"', "");
                    for prefix in ["%d", "${HOME}"] {
                        if path == prefix
                            || path
                                .strip_prefix(prefix)
                                .is_some_and(|tail| tail.starts_with('/'))
                        {
                            path = format!("~{}", &path[prefix.len()..]);
                        }
                    }
                    paths.push(path);
                    remaining = &remaining[end..];
                }
            }
        }
        if let Some(effect) = effect {
            for path in paths {
                context.add(&path, Some(index), effect, (letter == 'o').then_some(false));
            }
        }
        index += 1;
    }
}
