//! Wrapper option grammars, from the macOS manuals and the GNU manuals of the
//! wrappers that share their names. Where the two give a letter different
//! arities, it takes a value only when glued, and each value keeps the
//! strictest role either gives it. Every wrapper stops at its first operand.

use crate::options::{Grammar, Syntax};
use crate::record::Effect::{Enter, Name, Read, Write};

pub(super) const ENV: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "0iv",
    flags: "--null --ignore-environment --debug --list-signal-handling --help --version",
    values: &[
        ("-C --chdir", Enter),
        ("-u --unset -S --split-string -P -a --argv0", Name),
    ],
    attached: &[("--default-signal --ignore-signal --block-signal", Name)],
    stops: true,
    complete: true,
};

pub(super) const XARGS: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "0oprtx",
    flags: "--null --open-tty --interactive --no-run-if-empty --verbose --exit --show-limits --help --version",
    values: &[
        ("-a --arg-file", Read),
        (
            "-d --delimiter -E -I -J -L -n --max-args -P --max-procs -R -S -s --max-chars --process-slot-var",
            Name,
        ),
    ],
    attached: &[("-e --eof -i --replace -l --max-lines", Name)],
    stops: true,
    complete: true,
};

// sudo, and doas, whose `-C` names a configuration file it reads.
pub(super) const PRIVILEGE: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "ABbEeHiKkLlNnPSsVv",
    flags: "--askpass --bell --background --edit --set-home --help --login --remove-timestamp --reset-timestamp --list --no-update --non-interactive --preserve-groups --stdin --shell --version --validate",
    values: &[
        ("-D --chdir", Enter),
        ("-C", Read),
        (
            "--close-from -g --group --host -p --prompt -R --chroot -r --role -t --type -T --command-timeout -U --other-user -u --user -a -c --login-class",
            Name,
        ),
    ],
    // `-h` alone asks for help.
    attached: &[("-h --preserve-env", Name)],
    stops: true,
    complete: true,
};

pub(super) const TIMEOUT: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "fpv",
    flags: "--foreground --preserve-status --verbose --help --version",
    values: &[("-k --kill-after -s --signal", Name)],
    attached: &[],
    stops: true,
    complete: true,
};

pub(super) const STDBUF: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "",
    flags: "--help --version",
    values: &[("-i --input -o --output -e --error", Name)],
    attached: &[],
    stops: true,
    complete: true,
};

pub(super) const CAFFEINATE: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "dimsu",
    flags: "",
    values: &[("-t -w", Name)],
    attached: &[],
    stops: true,
    complete: true,
};

// Any other single-dash word names an architecture.
pub(super) const ARCH: Grammar = Grammar {
    values: &[("-e -d -arch", Name)],
    stops: true,
    ..Grammar::partial(Syntax::Words, &[])
};

pub(super) const SCRIPT: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "adeFkpqr",
    flags: "",
    values: &[("-t -T", Name)],
    attached: &[],
    stops: true,
    complete: true,
};

pub(super) const TIME: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "ahlpqv",
    flags: "--append --portability --quiet --verbose --help --version",
    values: &[("-o --output", Write), ("-f --format", Name)],
    attached: &[],
    stops: true,
    complete: true,
};
