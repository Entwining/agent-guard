//! Reader option grammars, from the macOS manuals and the GNU manuals of the
//! programs that share their names. Where the two give a letter different
//! arities, it takes a value only when glued; each value keeps the strictest
//! role either implementation gives it. Where macOS stops at the first
//! operand, the grammar does too.

use crate::options::{Grammar, Syntax};
use crate::record::Effect::{Name, Read, Use, Write};

pub(super) const SED: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "EaHnrsuz",
    flags: "--quiet --silent --debug --follow-symlinks --posix --regexp-extended --separate --sandbox --unbuffered --null-data --zero-terminated --help --version",
    // A script, like the script operand, keeps the read role.
    values: &[
        ("-e --expression -f --file", Read),
        ("-I --line-length", Name),
    ],
    attached: &[("-i --in-place -l", Name)],
    stops: true,
    complete: true,
};

pub(super) const AWK: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "bcCghIkMnNOPrsStV",
    flags: "-safe -version --characters-as-bytes --traditional --copyright --csv --gen-pot --help --trace --bignum --non-decimal-data --use-lc-numeric --optimize --no-optimize --posix --re-interval --sandbox --lint-old --version",
    values: &[
        (
            "-f --file -e --source -E --exec -i --include -l --load",
            Read,
        ),
        ("-F --field-separator -v --assign -W", Name),
    ],
    attached: &[
        ("-d --dump-variables -o --pretty-print -p --profile", Write),
        ("-D --debug", Read),
        ("-L --lint", Name),
    ],
    stops: true,
    complete: true,
};

pub(super) const SORT: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "bcCdfghiMmnRrsuVz",
    flags: "--merge --unique --stable --ignore-leading-blanks --dictionary-order --ignore-case --general-numeric-sort --human-numeric-sort --ignore-nonprinting --month-sort --numeric-sort --random-sort --reverse --version-sort --zero-terminated --debug --radixsort --mergesort --qsort --heapsort --mmap --help --version",
    values: &[
        ("-o --output -T --temporary-directory", Write),
        ("--files0-from --random-source", Read),
        ("--compress-program", Use),
        (
            "-k --key -t --field-separator -S --buffer-size --sort --batch-size --parallel",
            Name,
        ),
    ],
    attached: &[("--check", Name)],
    stops: false,
    complete: true,
};

pub(super) const DIFF: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "aBbcdEefHilNnPpqrsTtuvwyZ",
    flags: "--brief --ed --expand-tabs --forward-ed --ignore-all-space --ignore-case --ignore-space-change --ignore-blank-lines --ignore-tab-expansion --ignore-trailing-space --initial-tab --minimal --no-dereference --normal --rcs --show-c-function --speed-large-files --strip-trailing-cr --text --unidirectional-new-file --new-file --recursive --report-identical-files --side-by-side --suppress-common-lines --suppress-blank-empty --left-column --paginate --ignore-file-name-case --no-ignore-file-name-case --help --version",
    values: &[
        ("-X --exclude-from --from-file --to-file", Read),
        (
            "-A --algorithm -C -D --ifdef -F --show-function-line -I --ignore-matching-lines -L --label -S --starting-file -U -W --width -x --exclude --tabsize --changed-group-format --new-group-format --old-group-format --unchanged-group-format --line-format --new-line-format --old-line-format --unchanged-line-format --horizon-lines --palette",
            Name,
        ),
    ],
    attached: &[("--context --unified --color", Name)],
    stops: false,
    complete: true,
};

pub(super) const BASE64: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "dDh",
    flags: "--decode --ignore-garbage --help --version",
    values: &[
        ("--input", Read),
        ("-o --output", Write),
        ("-b --break -w --wrap", Name),
    ],
    // macOS reads `-i FILE`; GNU takes `-i` as a flag.
    attached: &[("-i", Read)],
    stops: false,
    complete: true,
};

pub(super) const HEXDUMP: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "bcCdovxhV",
    flags: "--one-byte-octal --one-byte-hex --one-byte-char --canonical --two-bytes-decimal --two-bytes-octal --two-bytes-hex --no-squeezing --help --version",
    values: &[
        ("-f --format-file", Read),
        ("-e --format -n --length -s --skip", Name),
    ],
    attached: &[("-L --color", Name)],
    stops: true,
    complete: true,
};

// OpenSSL commands share these output options; the others keep the read role.
pub(super) const OPENSSL: Grammar =
    Grammar::partial(Syntax::Words, &[("-out -keyout -writerand", Write)]);
