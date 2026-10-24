//! Where a shell invocation reads its commands, decided by each shell
//! family's own invocation grammar: bash(1) OPTIONS and ARGUMENTS, zsh(1)
//! INVOCATION with zshoptions(1) single letter options, dash(1) and ksh(1)
//! invocation options, and tcsh(1) argument list processing.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Bash,
    Zsh,
    Dash,
    Ksh,
    Csh,
}

pub(crate) struct ShellInvocation {
    pub source: ShellSource,
    /// Where the arguments after option processing start; with an
    /// uncertain source, the first word the grammar cannot place.
    pub operands: usize,
    /// Values that options take before `operands`, with the resource effect
    /// each option gives its value; `None` is data.
    pub values: Vec<(usize, Option<Effect>)>,
    /// Whether the code is POSIX shell code.
    pub posix: bool,
    #[cfg(test)]
    steps: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShellSource {
    /// `-c` is fixed; the code is this argument, absent when none follows.
    Code(Option<usize>),
    /// Commands come from the standard input.
    Stdin,
    /// This argument names the script.
    Script(usize),
    /// A word the grammar cannot place may select another mode, take the
    /// next word or end the options. These are the arguments that some
    /// reading of such words makes the code.
    Uncertain(Vec<usize>),
}

/// What the shell takes an argument as, given the modes earlier options set.
#[derive(Clone, Copy, PartialEq, Eq)]
struct State {
    expect: Expect,
    mode: Mode,
    stdin: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect {
    Option,
    /// The value of an option that gives it this resource effect.
    Value(Option<Effect>),
    /// The code that tcsh's `-c` takes.
    Code,
    /// The first operand, after the options have ended.
    Operand,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Script,
    /// `-c` makes the first operand the code.
    Code,
    /// tcsh's `-c` has taken its code.
    CodeTaken,
}

/// The source selection of `program`, a shell's base name, given the
/// arguments after it.
pub(crate) fn shell_invocation(program: &str, args: &[Word]) -> Option<ShellInvocation> {
    let family = match program {
        "bash" | "sh" => Family::Bash,
        "zsh" => Family::Zsh,
        "dash" => Family::Dash,
        "ksh" => Family::Ksh,
        "csh" | "tcsh" => Family::Csh,
        _ => return None,
    };
    // Each reading of the arguments walks through these states. Readings in
    // the same state at the same argument continue alike, so they merge and
    // the work stays linear in the arguments.
    let start = State {
        expect: Expect::Option,
        mode: Mode::Script,
        stdin: false,
    };
    let mut walk = Walk {
        family,
        states: vec![start],
        next: Vec::new(),
        end: (None, start),
        code: Vec::new(),
    };
    let mut unproven = None;
    let mut values = Vec::new();
    #[cfg(test)]
    let mut steps = 0;
    for (index, word) in args.iter().enumerate() {
        for state in std::mem::take(&mut walk.states) {
            #[cfg(test)]
            {
                steps += 1;
            }
            if !walk.step(word, index, state) {
                unproven.get_or_insert(index);
            } else if unproven.is_none()
                && let Expect::Value(effect) = state.expect
            {
                values.push((index, effect));
            }
        }
        walk.states = std::mem::take(&mut walk.next);
    }
    for state in std::mem::take(&mut walk.states) {
        walk.end(None, state);
    }
    let (operand, state) = walk.end;
    let source = if unproven.is_some() {
        ShellSource::Uncertain(walk.code)
    } else {
        match state.mode {
            Mode::Code => ShellSource::Code(operand),
            Mode::CodeTaken => ShellSource::Code(walk.code.last().copied()),
            Mode::Script if state.stdin => ShellSource::Stdin,
            Mode::Script => operand.map_or(ShellSource::Stdin, ShellSource::Script),
        }
    };
    Some(ShellInvocation {
        source,
        operands: unproven.or(operand).unwrap_or(args.len()),
        values,
        posix: family != Family::Csh,
        #[cfg(test)]
        steps,
    })
}

struct Walk {
    family: Family,
    states: Vec<State>,
    next: Vec<State>,
    /// The state of the last reading to end its options, and its first
    /// operand; with no unplaced word, the only reading.
    end: (Option<usize>, State),
    code: Vec<usize>,
}

impl Walk {
    fn continues(&mut self, state: State) {
        if !self.next.contains(&state) {
            self.next.push(state);
        }
    }

    fn end(&mut self, operand: Option<usize>, mut state: State) {
        if state.expect == Expect::Code {
            state.mode = Mode::CodeTaken;
        }
        if let Some(operand) = operand.filter(|_| state.mode == Mode::Code) {
            self.code_at(operand);
        }
        self.end = (operand, state);
    }

    /// Arguments are taken in order, so a repeated index is the last one.
    fn code_at(&mut self, index: usize) {
        if self.code.last() != Some(&index) {
            self.code.push(index);
        }
    }

    /// Takes the word at `index` in `state`; false when the grammar cannot
    /// place it.
    fn step(&mut self, word: &Word, index: usize, state: State) -> bool {
        match state.expect {
            Expect::Value(_) | Expect::Code if word.field_count_unknown => {
                // Its fields may be none, leaving the value to the next
                // word, or the value and words the grammar cannot place.
                self.continues(state);
                let mut after = State {
                    expect: Expect::Option,
                    ..state
                };
                if state.expect == Expect::Code {
                    self.code_at(index);
                    after.mode = Mode::CodeTaken;
                }
                self.unplaced(index, after, true);
                false
            }
            Expect::Value(_) => {
                self.continues(State {
                    expect: Expect::Option,
                    ..state
                });
                true
            }
            Expect::Code => {
                self.code_at(index);
                self.continues(State {
                    expect: Expect::Option,
                    mode: Mode::CodeTaken,
                    ..state
                });
                true
            }
            Expect::Operand if word.field_count_unknown => {
                self.continues(state);
                self.end(Some(index), state);
                false
            }
            Expect::Operand => {
                self.end(Some(index), state);
                true
            }
            Expect::Option => self.option(word, index, state),
        }
    }

    fn option(&mut self, word: &Word, index: usize, state: State) -> bool {
        let family = self.family;
        // tcsh reads only words starting with `-` as options.
        let option_sign = |c: char| c == '-' || c == '+' && family != Family::Csh;
        if word.field_count_unknown {
            self.unplaced(index, state, true);
            return false;
        }
        let Some(text) = word.exact() else {
            let prefix = word.known_prefix();
            if !prefix.is_empty() && !prefix.starts_with(option_sign) {
                self.end(Some(index), state);
                return true;
            }
            self.unplaced(index, state, prefix.is_empty());
            return false;
        };
        let terminator = match family {
            Family::Zsh => matches!(text, "-" | "+" | "--" | "+-"),
            Family::Bash | Family::Dash => matches!(text, "-" | "--"),
            // ksh ignores a `-` as the first argument.
            Family::Ksh => text == "--" || text == "-" && index > 0,
            Family::Csh => false,
        };
        if terminator {
            self.continues(State {
                expect: Expect::Operand,
                ..state
            });
            return true;
        }
        if text == "-" {
            if family == Family::Ksh {
                self.continues(state);
                return true;
            }
            self.unplaced(index, state, true);
            return false;
        }
        let Some(sign) = text.chars().next().filter(|c| option_sign(*c)) else {
            self.end(Some(index), state);
            return true;
        };
        if let Some(name) = text.strip_prefix("--").or(text.strip_prefix("+-")) {
            return self.long_option(name, sign, index, state);
        }
        let (flags, value_letters) = letters(family, sign);
        let letters = &text[1..];
        let mut next = state;
        let mut ends = false;
        for (at, letter) in letters.char_indices() {
            let rest = &letters[at + letter.len_utf8()..];
            if let Some((_, effect)) = value_letters.iter().find(|(value, _)| *value == letter) {
                // zsh `-o` and tcsh `-D` may take the rest of the word.
                if matches!(family, Family::Zsh | Family::Csh) && !rest.is_empty() {
                    break;
                }
                if !rest.is_empty() || family == Family::Csh {
                    self.unplaced(index, state, false);
                    return false;
                }
                next.expect = Expect::Value(*effect);
                continue;
            }
            if !flags.contains(letter) {
                self.unplaced(index, state, false);
                return false;
            }
            match (family, letter) {
                // tcsh reads commands from the following argument.
                (Family::Csh, 'c') => next.expect = Expect::Code,
                (_, 'c') => next.mode = Mode::Code,
                (_, 's') => next.stdin = true,
                (Family::Zsh | Family::Csh, 'b') => ends = true,
                (Family::Zsh, '-') if at > 0 && rest.is_empty() => ends = true,
                (Family::Zsh, '-') => {
                    self.unplaced(index, state, false);
                    return false;
                }
                _ => {}
            }
        }
        if ends {
            if next.expect != Expect::Option {
                self.unplaced(index, state, false);
                return false;
            }
            next.expect = Expect::Operand;
        }
        self.continues(next);
        true
    }

    fn long_option(&mut self, name: &str, sign: char, index: usize, state: State) -> bool {
        let next = match self.family {
            Family::Bash
                if sign == '-'
                    && [
                        "debugger",
                        "dump-po-strings",
                        "dump-strings",
                        "help",
                        "login",
                        "noediting",
                        "noprofile",
                        "norc",
                        "posix",
                        "restricted",
                        "verbose",
                        "version",
                    ]
                    .contains(&name) =>
            {
                state
            }
            // An interactive shell executes this startup file.
            Family::Bash if sign == '-' && ["init-file", "rcfile"].contains(&name) => State {
                expect: Expect::Value(Some(Effect::Read)),
                ..state
            },
            // Long options name options and take no value; emulation
            // changes what the single letters mean.
            Family::Zsh if name != "emulate" => state,
            Family::Csh if ["help", "version"].contains(&name) => state,
            _ => {
                self.unplaced(index, state, false);
                return false;
            }
        };
        self.continues(next);
        true
    }

    /// Every role a word the grammar cannot place may take: options setting
    /// any mode, taking the next word or ending the options, and the first
    /// operand when the word need not spell an option.
    fn unplaced(&mut self, index: usize, state: State, operand: bool) {
        let code = match (self.family, state.mode) {
            (Family::Csh, _) | (_, Mode::Code | Mode::CodeTaken) => state.mode,
            _ => Mode::Code,
        };
        let expects: &[Expect] = if self.family == Family::Csh {
            &[
                Expect::Option,
                Expect::Value(None),
                Expect::Operand,
                Expect::Code,
            ]
        } else {
            &[Expect::Option, Expect::Value(None), Expect::Operand]
        };
        for mode in [state.mode, code] {
            for stdin in [state.stdin, true] {
                for &expect in expects {
                    self.continues(State {
                        expect,
                        mode,
                        stdin,
                    });
                }
            }
        }
        if operand {
            self.end(Some(index), state);
        }
    }
}

/// The single letter flags a family accepts after `sign`, and the letters
/// that take a value with the resource effect they give it.
fn letters(family: Family, sign: char) -> (&'static str, &'static [(char, Option<Effect>)]) {
    match (family, sign) {
        (Family::Bash, '-') => ("abefhkmnptuvxBCEHPTilrDcs", &[('o', None), ('O', None)]),
        (Family::Bash, _) => ("abefhkmnptuvxBCEHPT", &[('o', None), ('O', None)]),
        (Family::Zsh, _) => (
            "0123456789BCDEFGHIJKLMNOPQRSTUVWXYZaefghiklmnprstuvwxycsb-",
            &[('o', None)],
        ),
        (Family::Dash, '-') => ("aCefnuvxIimqVEbcs", &[('o', None)]),
        (Family::Dash, _) => ("aCefnuvxIimqVEb", &[('o', None)]),
        // `-R` writes a cross reference database to the named file.
        (Family::Ksh, '-') => (
            "abefhikmnprstuvxBCDPEcs",
            &[('o', None), ('R', Some(Effect::Write))],
        ),
        (Family::Ksh, _) => ("abefhikmnprstuvxBCDP", &[('o', None)]),
        (Family::Csh, _) => ("bdefFilmnqstvVxXcs", &[('D', None)]),
    }
}

#[cfg(test)]
mod tests;
