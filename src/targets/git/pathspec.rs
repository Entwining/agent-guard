/// A pathspec written with git's magic prefix, as listed in `pathspec.c`.
pub(super) struct Magic<'a> {
    pub path: &'a str,
    pub exclude: bool,
}

pub(super) fn parse(word: &str) -> Option<Magic<'_>> {
    let rest = word.strip_prefix(':')?;
    let mut magic = if let Some(long) = rest.strip_prefix('(') {
        let (words, path) = long.split_once(')')?;
        // The other magic words are ignored; the path is checked as written.
        Magic {
            path,
            exclude: words.split(',').any(|name| name == "exclude"),
        }
    } else {
        let end = rest
            .find(|c| !matches!(c, '/' | '!' | '^'))
            .unwrap_or(rest.len());
        Magic {
            path: rest[end..].strip_prefix(':').unwrap_or(&rest[end..]),
            exclude: rest[..end].contains(['!', '^']),
        }
    };
    if magic.path.is_empty() {
        magic.path = ".";
    }
    Some(magic)
}
