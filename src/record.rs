//! Semantic records carried between the shell, target and rule owners.

pub mod stream;

#[derive(Clone, Copy)]
pub struct HostFacts<'a> {
    pub home: &'a str,
    pub user: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Word {
    pub text: String,
    pub raw: String,
    pub expands: bool,
    pub runtime_unknown: bool,
    pub globs: bool,
    pub shell_matches: bool,
    pub binding_candidates: std::collections::BTreeMap<String, String>,
    // Quoting fixes argv width while the value's repetition count stays unknown.
    pub cardinality_unknown: bool,
    pub field_count_unknown: bool,
    // Every source word up to this one expands to a known number of argv words,
    // so its argv position is exact.
    pub fixed_position: bool,
    pub vars: Vec<String>,
    pub role: Role,
    pub value: String,
    pub pwd: bool,
    pub cwd_ranges: Vec<std::ops::Range<usize>>,
    pub quoted_ranges: Vec<std::ops::Range<usize>>,
    pub stream: Option<Box<StreamOutput>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamOutput {
    pub value: stream::Flow,
    pub known: Vec<String>,
    pub unknown: bool,
}

impl Word {
    pub fn literal(text: String) -> Self {
        Self {
            raw: text.clone(),
            value: text.clone(),
            text,
            expands: false,
            runtime_unknown: false,
            globs: false,
            shell_matches: false,
            binding_candidates: std::collections::BTreeMap::new(),
            cardinality_unknown: false,
            field_count_unknown: false,
            fixed_position: false,
            vars: Vec::new(),
            role: Role::Arg,
            pwd: false,
            cwd_ranges: Vec::new(),
            quoted_ranges: Vec::new(),
            stream: None,
        }
    }
    pub fn as_str(&self) -> &str {
        &self.text
    }
    pub fn with_text(&self, text: String) -> Self {
        let quoted_ranges = if let Some(start) = self.text.len().checked_sub(text.len())
            && self.text.ends_with(&text)
        {
            self.quoted_ranges
                .iter()
                .filter_map(|range| {
                    let left = range.start.max(start);
                    let right = range.end.min(self.text.len());
                    (left < right).then(|| left - start..right - start)
                })
                .collect()
        } else if text.ends_with(&self.text) {
            let start = text.len() - self.text.len();
            self.quoted_ranges
                .iter()
                .map(|range| range.start + start..range.end + start)
                .collect()
        } else if !self.text.is_empty()
            && let Some(start) = text.find(&self.text)
            && text[start + self.text.len()..].find(&self.text).is_none()
        {
            self.quoted_ranges
                .iter()
                .map(|range| range.start + start..range.end + start)
                .collect()
        } else if let Some(start) = self.text.find(&text)
            && self.text[start + text.len()..].find(&text).is_none()
        {
            self.quoted_ranges
                .iter()
                .filter_map(|range| {
                    let left = range.start.max(start);
                    let right = range.end.min(start + text.len());
                    (left < right).then(|| left - start..right - start)
                })
                .collect()
        } else {
            Vec::new()
        };
        Self {
            text: text.clone(),
            value: text,
            cwd_ranges: Vec::new(),
            quoted_ranges,
            pwd: false,
            ..self.clone()
        }
    }
    pub(crate) fn reproject_cwd(&mut self, cwd: &str) {
        let old_ranges = self.cwd_ranges.clone();
        let project = |at: usize| {
            let mut changed = at as isize;
            for range in &old_ranges {
                if range.end <= at {
                    changed += cwd.len() as isize - range.len() as isize;
                }
            }
            changed as usize
        };
        for range in &mut self.quoted_ranges {
            *range = project(range.start)..project(range.end);
        }
        let mut text = String::new();
        let mut value = String::new();
        let mut cursor = 0;
        for range in &mut self.cwd_ranges {
            text.push_str(&self.text[cursor..range.start]);
            value.push_str(&self.value[cursor..range.start]);
            cursor = range.end;
            let start = text.len();
            text.push_str(cwd);
            value.push_str(cwd);
            *range = start..text.len();
        }
        text.push_str(&self.text[cursor..]);
        value.push_str(&self.value[cursor..]);
        self.text = text;
        self.value = value;
    }
}
impl std::ops::Deref for Word {
    type Target = str;
    fn deref(&self) -> &str {
        &self.text
    }
}
impl PartialEq<&str> for Word {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}
impl PartialEq<str> for Word {
    fn eq(&self, other: &str) -> bool {
        self.text == other
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Arg,
    Assign,
    Precommand,
    Namespace,
    Program,
    ObservedShellCode,
    Pattern,
    Path,
    PatternFile,
    OptionArg,
    Glob,
    Option(OptionRole),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OptionRole {
    Flag,
    PatternFile,
    Arg,
    Glob,
    Name,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
    Heredoc,
    Herestring,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    pub pattern: Option<String>,
    pub stream: Option<Box<StreamOutput>>,
    pub direction: Direction,
    pub fd: i32,
    pub duplicate: bool,
    pub target: String,
    pub globs: bool,
    pub shell_matches: bool,
    pub expands: bool,
    pub runtime_unknown: bool,
    pub vars: Vec<String>,
}
impl Redirect {
    pub fn from_word(word: Word, direction: Direction, fd: i32, duplicate: bool) -> Self {
        Self {
            pattern: (word.globs || word.shell_matches)
                .then(|| crate::filesystem::shell_pattern(&word.text, &word.quoted_ranges)),
            stream: word.stream,
            direction,
            fd,
            duplicate,
            target: word.text,
            globs: word.globs,
            shell_matches: word.shell_matches,
            expands: word.expands,
            runtime_unknown: word.runtime_unknown,
            vars: if matches!(direction, Direction::Heredoc | Direction::Herestring) {
                word.vars
            } else {
                Vec::new()
            },
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Items {
    pub root: String,
    pub hidden: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stdin {
    None,
    Inherited,
    Data(Vec<usize>),
    Shell,
    Code,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    Recursive,
    Explicit,
    Files,
    Hidden,
    Help,
    Fixed,
    Include,
    Replace,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub function: bool,
    pub argv: Vec<Word>,
    pub redirects: Vec<Redirect>,
    pub cwd: String,
    pub program: Option<usize>,
    pub wrappers: Vec<String>,
    pub shell: bool,
    pub flags: Vec<Flag>,
    pub items: Option<Items>,
    pub stdin: Stdin,
    pub pipeline: Option<(usize, usize)>,
    pub nested: bool,
}
impl Command {
    pub fn unresolved(&self) -> bool {
        self.argv.iter().any(|w| w.expands)
            || self
                .redirects
                .iter()
                .any(|r| matches!(r.direction, Direction::In | Direction::Out) && r.expands)
    }
    pub fn variables(&self) -> impl Iterator<Item = &String> {
        self.argv
            .iter()
            .flat_map(|w| &w.vars)
            .chain(self.redirects.iter().flat_map(|r| &r.vars))
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    pub text: String,
    pub cwd: String,
}
#[derive(Debug, Default)]
pub struct Script {
    pub commands: Vec<Command>,
    pub uninspectable: Vec<Fragment>,
    pub parse_failed: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effect {
    Read,
    Write,
    Change,
    Name,
    Meta,
    List,
    Enter,
    Use,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Walk {
    None,
    Visible,
    Hidden,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Via {
    Operand,
    Redirect,
    Cwd,
    Scan,
    Tool,
    /// A search glob that filters files below an already checked root.
    Filter,
    Items,
    Option,
    Code,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub pattern: Option<String>,
    pub path: String,
    pub unresolved: String,
    pub glob: bool,
    pub glob_hidden: bool,
    pub effect: Effect,
    pub walk: Walk,
    pub sends: bool,
    pub expands: bool,
    pub runtime_unknown: bool,
    pub via: Via,
    pub search: bool,
    pub command: Option<usize>,
    pub relocation_destination: Option<std::rc::Rc<Target>>,
}

impl Target {
    pub(crate) fn pattern_path(&self) -> std::borrow::Cow<'_, str> {
        match &self.pattern {
            Some(pattern) => {
                std::borrow::Cow::Owned(crate::filesystem::normalize(pattern, "/", "/"))
            }
            None => std::borrow::Cow::Borrowed(&self.path),
        }
    }
    pub fn from_word(
        word: &Word,
        cwd: &str,
        host: HostFacts<'_>,
        effect: Effect,
        walk: Walk,
    ) -> Self {
        let input = if matches!(
            crate::shell::lexer::initial_quote(&word.raw),
            crate::shell::lexer::Quote::Single | crate::shell::lexer::Quote::Double
        ) {
            word.text.clone()
        } else {
            crate::filesystem::expand_home(&word.text, host.home, host.user)
        };
        let input = crate::filesystem::strip_file_url(&input);
        let path = if input.starts_with('/') {
            input.to_owned()
        } else {
            format!("{cwd}/{input}")
        };
        Self {
            pattern: (word.globs || word.shell_matches).then(|| {
                let value =
                    word.with_text(crate::filesystem::strip_file_url(&word.text).to_owned());
                let pattern = crate::filesystem::shell_pattern(&value.text, &value.quoted_ranges);
                let pattern = if matches!(
                    crate::shell::lexer::initial_quote(&word.raw),
                    crate::shell::lexer::Quote::Single | crate::shell::lexer::Quote::Double
                ) {
                    pattern
                } else {
                    crate::filesystem::expand_home(
                        &pattern,
                        &crate::filesystem::literal_shell_pattern(host.home),
                        host.user,
                    )
                };
                crate::filesystem::absolute_pattern(
                    crate::filesystem::strip_file_url(&pattern),
                    cwd,
                )
            }),
            glob: word.globs || word.shell_matches,
            glob_hidden: !word.globs && !word.shell_matches,
            expands: word.expands,
            runtime_unknown: word.runtime_unknown,
            ..Self::new(path, effect, walk, Via::Operand)
        }
    }
    pub fn new(path: String, effect: Effect, walk: Walk, via: Via) -> Self {
        let normalized = crate::filesystem::normalize(&path, "/", "/");
        Self {
            pattern: None,
            unresolved: path,
            path: normalized,
            glob: false,
            glob_hidden: true,
            effect,
            walk,
            via,
            expands: false,
            runtime_unknown: false,
            sends: false,
            search: false,
            command: None,
            relocation_destination: None,
        }
    }
}

#[cfg(test)]
mod tests;
