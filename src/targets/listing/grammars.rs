//! Listing option grammars: fd's own help, the macOS and GNU du manuals, and
//! the tree manual. A du value keeps the strictest role either du gives it,
//! and du stops at the first operand as macOS du does.

use crate::options::{Grammar, Syntax};
use crate::record::Effect::{Enter, List, Name, Read, Write};

pub(super) const FD: Grammar = Grammar {
    syntax: Syntax::Clap,
    short: "HIusigFalLp0q1hV",
    flags: "--hidden --no-hidden --no-ignore --ignore --unrestricted --no-ignore-vcs --ignore-vcs --no-require-git --require-git --no-ignore-parent --no-global-ignore-file --case-sensitive --ignore-case --glob --regex --fixed-strings --absolute-path --relative-path --list-details --follow --no-follow --full-path --print0 --quiet --has-results --show-errors --one-file-system --mount --xdev --prune --help --version",
    values: &[
        ("-C --base-directory", Enter),
        ("--search-path", List),
        ("--ignore-file", Read),
        (
            "-c --color -d --max-depth --min-depth --exact-depth -t --type -e --extension -E --exclude --ignore-contain -j --threads -S --size --changed-within --change-newer-than --newer --changed-after --changed-before --change-older-than --older -o --owner --path-separator --format --batch-size --and --max-results",
            Name,
        ),
    ],
    attached: &[("--strip-cwd-prefix --hyperlink", Name)],
    stops: false,
    complete: true,
};

pub(super) const DU: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "0aAbcDgHhkLlmnPrSsx",
    flags: "--null --all --apparent-size --bytes --total --dereference-args --human-readable --inodes --dereference --count-links --no-dereference --separate-dirs --si --summarize --one-file-system --help --version",
    // du prints the names a list file holds, in its error messages too.
    values: &[
        ("-X --exclude-from --files0-from", Read),
        (
            "-B --block-size -d --max-depth -I -t --threshold --exclude --time-style",
            Name,
        ),
    ],
    attached: &[("--time --libxo", Name)],
    stops: true,
    complete: true,
};

pub(super) const TREE: Grammar = Grammar {
    syntax: Syntax::Getopt,
    short: "adlfxRqNQpugshDFvtcUriASnCXJ",
    flags: "--gitignore --ignore-case --matchdirs --metafirst --prune --info --noreport --si --du --inodes --device --dirsfirst --filesfirst --nolinks --fromfile --fromtabfile --fflinks --opt-toggle --help --version",
    values: &[
        ("-o", Write),
        ("--gitfile --infofile --hintro --houtro", Read),
        (
            "-L -P -I -H -T --charset --filelimit --timefmt --sort",
            Name,
        ),
    ],
    attached: &[],
    stops: false,
    complete: true,
};
