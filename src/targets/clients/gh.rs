use super::*;

// The options of gh 2.105's `search` commands and `gh api`, from `gh help`.
// No search option reads a local file: queries, filters and file names are
// what the search matches on GitHub.
const SEARCH_CODE: Grammar = complete(
    &[(
        "--extension --filename -q --jq --json --language -L --limit --match --owner -R --repo --size -t --template",
        Name,
    )],
    &[("-w --web --help", Name)],
);
const SEARCH_COMMITS: Grammar = complete(
    &[(
        "--author --author-date --author-email --author-name --committer --committer-date --committer-email --committer-name --hash -q --jq --json -L --limit --order --owner --parent -R --repo --sort -t --template --tree --visibility",
        Name,
    )],
    &[("--merge -w --web --help", Name)],
);
const SEARCH_ISSUES: Grammar = complete(
    &[(
        "--app --assignee --author --closed --commenter --comments --created --interactions --involves -q --jq --json --label --language -L --limit --match --mentions --milestone --order --owner --project --reactions -R --repo --search-type --sort --state --team-mentions -t --template --updated --visibility",
        Name,
    )],
    &[(
        "--archived --include-prs --locked --no-assignee --no-label --no-milestone --no-project -w --web --help",
        Name,
    )],
);
const SEARCH_PRS: Grammar = complete(
    &[(
        "--app --assignee --author -B --base --checks --closed --commenter --comments --created -H --head --interactions --involves -q --jq --json --label --language -L --limit --match --mentions --merged-at --milestone --order --owner --project --reactions -R --repo --review --review-requested --reviewed-by --sort --state --team-mentions -t --template --updated --visibility",
        Name,
    )],
    &[(
        "--archived --draft --locked --merged --no-assignee --no-label --no-milestone --no-project -w --web --help",
        Name,
    )],
);
const SEARCH_REPOS: Grammar = complete(
    &[(
        "--created --followers --forks --good-first-issues --help-wanted-issues --include-forks -q --jq --json --language --license -L --limit --match --number-topics --order --owner --size --sort --stars -t --template --topic --updated --visibility",
        Name,
    )],
    &[("--archived -w --web --help", Name)],
);
// `--input FILE` and `-F key=@FILE` read files, which `api` below resolves;
// `-f key=@FILE` sends the text literally.
const API: Grammar = complete(
    &[(
        "--cache -F --field -H --header --hostname --input -q --jq -X --method -p --preview -f --raw-field -t --template",
        Name,
    )],
    &[(
        "--allow-escape-sequences -i --include --paginate --silent --slurp --verbose --help",
        Name,
    )],
);

/// A grammar of every option of a command, in pflag's syntax, where a
/// boolean takes a value only when glued, as `--web=false`.
const fn complete(
    values: &'static [(&'static str, Effect)],
    attached: &'static [(&'static str, Effect)],
) -> Grammar {
    Grammar {
        values,
        attached,
        complete: true,
        ..Grammar::partial(Syntax::Clap, &[])
    }
}

/// The roles of a gh command whose options are modelled, named by its
/// leading words; any other command stays an unknown program.
pub(super) fn spec(words: &[Word]) -> Option<Spec> {
    let mut path = words.iter().map(Word::exact);
    let grammar = match (path.next()??, path.next().flatten()) {
        ("api", _) => &API,
        ("search", Some("code")) => &SEARCH_CODE,
        ("search", Some("commits")) => &SEARCH_COMMITS,
        ("search", Some("issues")) => &SEARCH_ISSUES,
        ("search", Some("prs")) => &SEARCH_PRS,
        ("search", Some("repos")) => &SEARCH_REPOS,
        _ => return None,
    };
    Some(Spec {
        operand: Name,
        walk: Walk::None,
        options: Some(grammar),
        remote: false,
        destination: None,
    })
}

/// Adds the files `gh api` reads: the `--input` body and each `-F` value
/// spelled `@FILE`, other than `-` for standard input.
pub(super) fn api(context: &mut Context<'_>) {
    if context.words.first().and_then(Word::exact) != Some("api") {
        return;
    }
    let binding = crate::options::bind(context.words, &API);
    for bound in binding.values {
        let value = bound.value.word(context.words);
        let file = match bound.name {
            "--input" => Some(value),
            "-F" | "--field" => {
                // Text the shell supplies before `=` could hold the `=@`.
                match value.known_prefix().find('=') {
                    Some(at) if value.known_prefix()[at + 1..].starts_with('@') => {
                        Some(value.suffix(at + 2))
                    }
                    Some(at) if value.known_prefix().len() == at + 1 && value.exact().is_none() => {
                        Some(value.suffix(at + 1))
                    }
                    Some(_) => None,
                    None => value.exact().is_none().then_some(value),
                }
            }
            _ => None,
        };
        if let Some(file) = file.filter(|file| file.exact() != Some("-")) {
            let mut target = Target::from_word(&file, context.cwd, context.host, Read, Walk::None);
            target.via = Via::Option;
            target.sends = true;
            context.targets.push(target);
        }
    }
}
