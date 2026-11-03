//! Option grammars: which argument words a program reads as options, which
//! word holds each option's value, and the role the program gives that value.
//! The binder knows only syntax; each program's owner declares its grammar
//! and decides what its values and operands mean.

use crate::record::{Effect, OptionValue, Word};

/// How a parser spells options and their values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Syntax {
    /// getopt_long: `-abc` groups short flags, a short option's value is glued
    /// as `-oVALUE` or is the next word, and a long option's value is glued
    /// as `--name=VALUE` or is the next word; a long name may be abbreviated.
    Getopt,
    /// clap and pflag: as getopt, except that `-o=VALUE` gives `VALUE` and
    /// long names are never abbreviated.
    Clap,
    /// Every option is one whole word, such as OpenSSL's `-out`, whose value
    /// is the next word.
    Words,
}

pub(crate) struct Grammar {
    pub syntax: Syntax,
    /// The short flags, as letters.
    pub short: &'static str,
    /// The long flags, and the flags that are one whole single-dash word,
    /// such as every flag of the `Words` syntax.
    pub flags: &'static str,
    /// The options that take a value, by space-separated names, with the role
    /// of their value; `Effect::Name` marks data.
    pub values: &'static [(&'static str, Effect)],
    /// The options that take a value only when it is glued to them, and are
    /// flags otherwise, such as `-i[SUFFIX]` or `--color[=WHEN]`.
    pub attached: &'static [(&'static str, Effect)],
    /// Whether the first operand ends the options, as in BSD getopt.
    pub stops: bool,
    /// Whether the grammar lists every option of the program. An option a
    /// partial grammar does not list counts as a flag; one a complete grammar
    /// does not list leaves the roles of the words after it unproven.
    pub complete: bool,
}

impl Grammar {
    /// A grammar that models only the value options `values`.
    pub(crate) const fn partial(syntax: Syntax, values: &'static [(&'static str, Effect)]) -> Self {
        Self {
            syntax,
            short: "",
            flags: "",
            values,
            attached: &[],
            stops: false,
            complete: false,
        }
    }
}

/// A value an option took, by the name the grammar lists for that option.
#[derive(Debug, Clone)]
pub(crate) struct Bound {
    pub name: &'static str,
    pub value: OptionValue,
}

#[derive(Debug, Default)]
pub(crate) struct Binding {
    pub values: Vec<Bound>,
    /// The flags given, by name: a grouped short flag as `-x`.
    pub flags: Vec<String>,
    /// The option words, the words holding their values, and `--`.
    pub options: Vec<usize>,
    pub operands: Vec<usize>,
    /// The first word whose role the grammar cannot prove: an option it does
    /// not know, runtime text that could spell an option, or a value word the
    /// shell may split.
    pub unproven: Option<usize>,
}

fn named(names: &str, name: &str) -> bool {
    names.split_whitespace().any(|listed| listed == name)
}

fn find(table: &'static [(&'static str, Effect)], name: &str) -> Option<(&'static str, Effect)> {
    table.iter().find_map(|(names, effect)| {
        names
            .split_whitespace()
            .find(|listed| *listed == name)
            .map(|listed| (listed, *effect))
    })
}

/// The long option `name` spells. getopt_long also accepts any prefix of
/// exactly one long option; a prefix of several is an error, and stays
/// unlisted.
fn long_name<'a>(grammar: &Grammar, name: &'a str) -> &'a str {
    let longs = || {
        grammar
            .values
            .iter()
            .chain(grammar.attached)
            .flat_map(|(names, _)| names.split_whitespace())
            .chain(grammar.flags.split_whitespace())
            .filter(|listed| listed.starts_with("--"))
    };
    if grammar.syntax != Syntax::Getopt || name.len() < 3 || longs().any(|listed| listed == name) {
        return name;
    }
    let mut extensions = longs().filter(|listed| listed.starts_with(name));
    match (extensions.next(), extensions.next()) {
        (Some(only), None) => only,
        _ => name,
    }
}

impl Binding {
    pub(crate) fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|flag| flag == name)
    }

    /// Records an option `grammar` does not list, the last word recorded. A
    /// partial grammar leaves it to the program's generic roles.
    fn unknown(&mut self, at: usize, grammar: &Grammar) {
        if grammar.complete {
            self.unproven.get_or_insert(at);
        } else {
            self.options.pop();
        }
    }

    fn bind(&mut self, name: &'static str, effect: Effect, index: usize, prefix: usize) {
        self.values.push(Bound {
            name,
            value: OptionValue {
                index,
                prefix,
                effect,
            },
        });
    }

    /// Binds a value that is the word after the option at `at`, and returns
    /// how many words the option spans.
    fn next(&mut self, args: &[Word], at: usize, name: &'static str, effect: Effect) -> usize {
        let Some(value) = args.get(at + 1) else {
            return 1;
        };
        if !value.fixed_position || value.cardinality_unknown {
            self.unproven.get_or_insert(at + 1);
        }
        self.bind(name, effect, at + 1, 0);
        self.options.push(at + 1);
        2
    }

    /// Binds the option at `at` when the parser reads that word as one, and
    /// returns how many words it spans; `None` leaves the word an operand.
    /// The caller handles `--`.
    pub(crate) fn option(&mut self, args: &[Word], at: usize, grammar: &Grammar) -> Option<usize> {
        let word = args.get(at)?;
        let exact = word.exact();
        let text = exact.unwrap_or_else(|| word.known_prefix());
        if exact == Some("-") || !text.starts_with('-') {
            if word.option_unknown() {
                self.unproven.get_or_insert(at);
            }
            return None;
        }
        self.options.push(at);
        if grammar.syntax == Syntax::Words {
            if exact.is_none() {
                self.unproven.get_or_insert(at);
            } else if let Some((name, effect)) = find(grammar.values, text) {
                return Some(self.next(args, at, name, effect));
            } else if named(grammar.flags, text) {
                self.flags.push(text.to_owned());
            } else {
                self.unknown(at, grammar);
            }
            return Some(1);
        }
        if text.starts_with("--") {
            let (spelled, glued) = match text.split_once('=') {
                Some((spelled, _)) => (spelled, Some(spelled.len() + 1)),
                None => (text, None),
            };
            // A partial grammar omits the program's flags, so only a value
            // glued with `=`, which no flag accepts, proves that an
            // abbreviation names a listed option.
            let name = if grammar.complete || glued.is_some() {
                long_name(grammar, spelled)
            } else {
                spelled
            };
            // Runtime text completes the name unless `=` already ended it.
            if exact.is_none() && glued.is_none() {
                self.unproven.get_or_insert(at);
            } else if let Some((name, effect)) = find(grammar.values, name) {
                return Some(match glued {
                    Some(prefix) => {
                        self.bind(name, effect, at, prefix);
                        1
                    }
                    None => self.next(args, at, name, effect),
                });
            } else if let Some((name, effect)) = find(grammar.attached, name) {
                match glued {
                    Some(prefix) => self.bind(name, effect, at, prefix),
                    None => self.flags.push(name.to_owned()),
                }
            } else if glued.is_none() && named(grammar.flags, name) {
                self.flags.push(name.to_owned());
            } else {
                self.unknown(at, grammar);
            }
            return Some(1);
        }
        // A single-dash flag such as awk's `-safe` is one whole word.
        if exact.is_some() && named(grammar.flags, text) {
            self.flags.push(text.to_owned());
            return Some(1);
        }
        let mut known = false;
        for (offset, letter) in text.char_indices().skip(1) {
            let short = format!("-{letter}");
            let mut prefix = offset + letter.len_utf8();
            if grammar.syntax == Syntax::Clap && text[prefix..].starts_with('=') {
                prefix += 1;
            }
            // Runtime text after a known option may be empty, leaving the
            // value to the next word.
            let glued = prefix < text.len() || exact.is_none();
            if exact.is_none() && prefix == text.len() {
                self.unproven.get_or_insert(at);
            }
            if let Some((name, effect)) = find(grammar.values, &short) {
                if glued {
                    self.bind(name, effect, at, prefix);
                    return Some(1);
                }
                return Some(self.next(args, at, name, effect));
            }
            if let Some((name, effect)) = find(grammar.attached, &short) {
                if glued {
                    self.bind(name, effect, at, prefix);
                } else {
                    self.flags.push(name.to_owned());
                }
                return Some(1);
            }
            if grammar.short.contains(letter) {
                known = true;
                self.flags.push(short);
            } else if grammar.complete {
                self.unknown(at, grammar);
                return Some(1);
            }
        }
        // Runtime text may continue the group with any option.
        if exact.is_none() {
            self.unproven.get_or_insert(at);
        }
        if !known {
            self.unknown(at, grammar);
        }
        Some(1)
    }
}

/// Where `grammar` places each word of `args`.
pub(crate) fn bind(args: &[Word], grammar: &Grammar) -> Binding {
    let mut binding = Binding::default();
    let mut at = 0;
    let mut options = true;
    while at < args.len() {
        if options {
            if args[at].exact() == Some("--") {
                binding.options.push(at);
                options = false;
                at += 1;
                continue;
            }
            if let Some(words) = binding.option(args, at, grammar) {
                at += words;
                continue;
            }
            options = !grammar.stops;
        }
        binding.operands.push(at);
        at += 1;
    }
    binding
}

#[cfg(test)]
mod tests;
