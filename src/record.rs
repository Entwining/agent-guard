//! Semantic records carried between the shell, target and rule owners.

#[derive(Clone, Copy)]
pub struct HostFacts<'a> {
    pub home: &'a str,
    pub user: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub text: String,
    pub raw: String,
    pub expands: bool,
    pub runtime_unknown: bool,
    pub globs: bool,
    pub shell_matches: bool,
    pub binding_candidates: std::collections::BTreeMap<String, String>,
    pub vars: Vec<String>,
    pub role: Role,
    pub value: String,
    pub pwd: bool,
    pub cwd_ranges: Vec<std::ops::Range<usize>>,
    pub stream: Option<Box<StreamOutput>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamOutput {
    Known(Vec<String>),
    Unknown,
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
            vars: Vec::new(),
            role: Role::Arg,
            pwd: false,
            cwd_ranges: Vec::new(),
            stream: None,
        }
    }
    pub fn as_str(&self) -> &str {
        &self.text
    }
    pub fn with_text(&self, text: String) -> Self {
        Self {
            text: text.clone(),
            value: text,
            cwd_ranges: Vec::new(),
            pwd: false,
            ..self.clone()
        }
    }
    pub(crate) fn reproject_cwd(&mut self, cwd: &str) {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Arg,
    Assign,
    Precommand,
    Namespace,
    Program,
    Code,
    Pattern,
    Path,
    PatternFile,
    OptionArg,
    Glob,
    Option(OptionRole),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub stream: Option<Box<StreamOutput>>,
    pub direction: Direction,
    pub target: String,
    pub globs: bool,
    pub shell_matches: bool,
    pub expands: bool,
    pub runtime_unknown: bool,
    pub vars: Vec<String>,
}
impl Redirect {
    pub fn from_word(word: Word, direction: Direction) -> Self {
        Self {
            stream: word.stream,
            direction,
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
    pub environment: Vec<(String, Word)>,
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
    Items,
    Option,
    Code,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
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
}

impl Target {
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
            glob: word.globs || word.shell_matches,
            glob_hidden: !word.globs && !word.shell_matches,
            expands: word.expands,
            runtime_unknown: word.runtime_unknown,
            ..Self::new(path, effect, walk, Via::Operand)
        }
    }
    pub fn new(path: String, effect: Effect, walk: Walk, via: Via) -> Self {
        Self {
            unresolved: path.clone(),
            path: crate::filesystem::normalize(&path, "/", "/"),
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
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rewritten_word_clears_cwd_projection_and_preserves_raw_provenance() {
        let mut old = super::Word::literal("/old".into());
        old.raw = "$(pwd)/old".into();
        old.pwd = true;
        old.cwd_ranges = std::iter::once(0..4).collect();
        let new = old.with_text("/new".into());
        assert_eq!((new.text.as_str(), new.value.as_str()), ("/new", "/new"));
        assert!(!new.pwd && new.cwd_ranges.is_empty());
        assert_eq!(new.raw, old.raw);
    }
    #[test]
    fn projected_cwd_ranges_describe_current_word_bytes() {
        let packet: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/rust-m2-cwd.json")).unwrap();
        let mut projected = 0;
        for row in packet["rows"].as_array().unwrap() {
            let result = crate::shell::observe(
                row["source"].as_str().unwrap(),
                crate::shell::Arm::Brush,
                "/h",
                row["cwd"].as_str().unwrap(),
                true,
            )
            .unwrap();
            for command in result.script.commands {
                for word in command.argv {
                    for range in word.cwd_ranges {
                        assert_eq!(
                            word.text.get(range.clone()),
                            Some(command.cwd.as_str()),
                            "{}",
                            row["id"]
                        );
                        assert_eq!(
                            word.value.get(range),
                            Some(command.cwd.as_str()),
                            "{}",
                            row["id"]
                        );
                        projected += 1;
                    }
                }
            }
        }
        assert!(projected > 0);
    }
}
