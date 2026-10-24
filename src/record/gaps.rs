use super::Word;

/// Runtime text the guard cannot determine, represented by `range` of the
/// word's text, which may be empty.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Gap {
    pub range: std::ops::Range<usize>,
    /// The runtime text is sure to be nonempty.
    pub nonempty: bool,
}

/// The unknown runs of a text that is either of two values with the same
/// representative: a byte is known only where both know it.
pub fn union_gaps(left: &[Gap], right: &[Gap]) -> Vec<Gap> {
    let mut gaps = left.iter().chain(right).cloned().collect::<Vec<_>>();
    gaps.sort_by_key(|gap| (gap.range.start, gap.range.end));
    let mut merged: Vec<Gap> = Vec::new();
    for gap in gaps {
        match merged.last_mut() {
            Some(last) if last.range == gap.range => last.nonempty &= gap.nonempty,
            Some(last) if gap.range.start < last.range.end => {
                last.range.end = last.range.end.max(gap.range.end);
                last.nonempty = false;
            }
            _ => merged.push(Gap {
                nonempty: gap.nonempty && !gap.range.is_empty(),
                ..gap
            }),
        }
    }
    merged
}

/// Whether a text of `len` bytes with the unknown runs `gaps` is sure to be
/// nonempty at run time: some byte is known, or some run is nonempty.
pub fn nonempty_text(len: usize, gaps: &[Gap]) -> bool {
    (0..len).any(|at| !gaps.iter().any(|gap| gap.range.contains(&at)))
        || gaps.iter().any(|gap| gap.nonempty)
}

impl Word {
    /// The unknown runs of the runtime text. Flags that no recorded run
    /// locates cover the whole word, and pathname expansion leaves only the
    /// text before its first pattern character known. An unknown field count
    /// is a width fact: each field it repeats keeps its own bytes.
    pub fn unknown(&self) -> std::borrow::Cow<'_, [Gap]> {
        let whole = || Gap {
            range: 0..self.text.len(),
            nonempty: false,
        };
        if self.gaps.is_empty() && (self.expands || self.runtime_unknown) {
            return std::borrow::Cow::Owned(vec![whole()]);
        }
        if let Some(start) = self.pattern_start() {
            let mut gaps = self
                .gaps
                .iter()
                .filter(|gap| gap.range.end <= start)
                .cloned()
                .collect::<Vec<_>>();
            let start = self
                .gaps
                .iter()
                .filter(|gap| gap.range.end > start)
                .map(|gap| gap.range.start)
                .fold(start, usize::min);
            gaps.push(Gap {
                range: start..self.text.len(),
                nonempty: false,
            });
            return std::borrow::Cow::Owned(gaps);
        }
        std::borrow::Cow::Borrowed(&self.gaps)
    }
    /// Where pathname expansion makes the text a pattern.
    fn pattern_start(&self) -> Option<usize> {
        if !self.globs && !self.shell_matches {
            return None;
        }
        // A bracket without a closing one is literal, as in the `[` builtin.
        self.text.char_indices().find_map(|(at, c)| {
            ("*?[{(".contains(c)
                && (c != '[' || self.text[at + 1..].contains(']'))
                && !self.quoted_ranges.iter().any(|r| r.contains(&at)))
            .then_some(at)
        })
    }
    /// The text, when the shell fixes every byte of it apart from the
    /// matches of its pathname pattern.
    pub fn pattern_exact(&self) -> Option<&str> {
        match self.unknown().as_ref() {
            [] => Some(&self.text),
            [gap] if Some(gap.range.start) == self.pattern_start() => Some(&self.text),
            _ => None,
        }
    }
    /// The runtime text, when the shell fixes every byte of it.
    pub fn exact(&self) -> Option<&str> {
        self.unknown().is_empty().then_some(self.text.as_str())
    }
    /// The literal text the runtime word is sure to start with.
    pub fn known_prefix(&self) -> &str {
        &self.text[..self
            .unknown()
            .first()
            .map_or(self.text.len(), |gap| gap.range.start)]
    }
    /// Whether the shell leaves open which option a parser reads this word
    /// as: text it does not fix could start with a dash, or could complete
    /// an option name. A fixed `--name=` prefix spells that option with a
    /// glued value. Pathname expansion keeps the known prefix in every match.
    pub fn option_unknown(&self) -> bool {
        let prefix = self.known_prefix();
        self.exact().is_none()
            && (prefix.is_empty()
                || prefix.starts_with('-')
                    && !prefix
                        .strip_prefix("--")
                        .and_then(|long| long.split_once('='))
                        .is_some_and(|(name, _)| !name.is_empty()))
    }
    /// The word from an offset of its text to its end, keeping its evidence
    /// unless the offset splits an unknown run, which leaves all of it
    /// unknown.
    pub fn suffix(&self, start: usize) -> Self {
        let mut word = self.with_text(self.text[start..].to_owned());
        let unknown = self.unknown();
        if unknown
            .iter()
            .any(|gap| gap.range.start < start && start < gap.range.end)
        {
            return word;
        }
        word.gaps = unknown
            .iter()
            .filter(|gap| gap.range.start >= start)
            .map(|gap| Gap {
                range: gap.range.start - start..gap.range.end - start,
                nonempty: gap.nonempty,
            })
            .collect();
        word.expands &= !word.gaps.is_empty();
        word.runtime_unknown &= !word.gaps.is_empty();
        word
    }
}
