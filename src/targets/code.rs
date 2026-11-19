mod scanner;
mod syntax;

use super::Target;
use crate::record::{Effect, HostFacts, Via, Walk, Word};
use scanner::Scanner;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineCode {
    pub text: String,
    pub syntax: Syntax,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syntax {
    /// Shell text no parser could read, such as a representative of runtime
    /// code; any token that could spell a path is a mention.
    ShellFragment,
    /// Source an interpreter runs, where file names live in string content.
    Interpreter(Language),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Python,
    JavaScript,
    Ruby,
    Perl,
    Php,
    Lua,
    AppleScript,
}

impl Language {
    pub fn of(program: &str, args: &[Word]) -> Option<Self> {
        Some(match program {
            "python" | "python3" => Self::Python,
            "node" | "bun" | "deno" | "tsx" => Self::JavaScript,
            "ruby" => Self::Ruby,
            "perl" => Self::Perl,
            "php" => Self::Php,
            "lua" => Self::Lua,
            "osascript"
                if args
                    .iter()
                    .zip(args.iter().skip(1))
                    .any(|(option, value)| option == "-l" && value == "JavaScript")
                    || args.iter().any(|arg| arg == "-lJavaScript") =>
            {
                Self::JavaScript
            }
            "osascript" => Self::AppleScript,
            _ => return None,
        })
    }
}

impl InlineCode {
    /// Interpreter source, or a shell fragment when the program's language is
    /// not modelled.
    pub fn interpreter(text: String, program: &str, args: &[Word]) -> Self {
        Self {
            text,
            syntax: Language::of(program, args).map_or(Syntax::ShellFragment, Syntax::Interpreter),
        }
    }
    pub fn shell_fragment(text: String) -> Self {
        Self {
            text,
            syntax: Syntax::ShellFragment,
        }
    }
}

/// Text in inline code that may name a file. It is not a proven read, but it
/// is judged as one so the resolver decides its protection whatever the code
/// does with the name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub text: String,
    pub pattern: bool,
}

impl Mention {
    pub fn target(&self, cwd: &str, host: HostFacts<'_>) -> Target {
        let mut target = Target::new(
            crate::filesystem::absolute_input(&self.text, cwd, host.home),
            Effect::Read,
            Walk::None,
            Via::Code,
        );
        if self.pattern {
            target.glob = true;
            target.glob_hidden = false;
            target.pattern = Some(crate::filesystem::absolute_pattern(
                &crate::filesystem::expand_home(
                    &self.text,
                    &crate::filesystem::literal_shell_pattern(host.home),
                    host.user,
                ),
                cwd,
            ));
        }
        target
    }
}

pub fn mentions(code: &InlineCode) -> Vec<Mention> {
    match code.syntax {
        Syntax::ShellFragment => shell_fragment_mentions(&code.text),
        Syntax::Interpreter(language) => Scanner::new(&code.text, language).scan(),
    }
}

fn path_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '.' | '/' | '~' | '_' | '-' | '$')
}

fn home_relative(token: &str) -> String {
    ["$HOME/", "${HOME}/"]
        .into_iter()
        .find(|prefix| {
            token
                .get(..prefix.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
        })
        .map_or_else(
            || token.to_owned(),
            |prefix| format!("~/{}", &token[prefix.len()..]),
        )
}

fn braced_home(chars: &[char], at: usize) -> bool {
    chars.get(at..at + 7).is_some_and(|prefix| {
        prefix
            .iter()
            .collect::<String>()
            .eq_ignore_ascii_case("${HOME}")
    })
}

// A shell fragment keeps the conservative token boundary: a dot-led token,
// a token with a slash, and any token in or beside quotes is a mention.
fn shell_fragment_mentions(code: &str) -> Vec<Mention> {
    let mut paths = Vec::new();
    let mut cursor = 0;
    let mut quote = 0;
    while cursor < code.len() {
        let ch = code[cursor..].chars().next().unwrap_or_default();
        if ch.is_alphanumeric() || matches!(ch, '.' | '/' | '~' | '_' | '$') {
            let start = cursor;
            // Keep the braced HOME prefix in one pathname token.
            if code[cursor..]
                .get(..7)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("${HOME}"))
            {
                cursor += 7;
            } else {
                cursor += ch.len_utf8();
            }
            while cursor < code.len() {
                let next = code[cursor..].chars().next().unwrap_or_default();
                if !path_char(next) {
                    break;
                }
                cursor += next.len_utf8();
            }
            let token = &code[start..cursor];
            if (start == 0 || code.as_bytes()[start - 1] != b'\\')
                && (token.contains('/')
                    || token.starts_with('.')
                    || quote != 0
                    || matches!(
                        code.as_bytes().get(start.wrapping_sub(1)),
                        Some(b'\'' | b'"' | b'`')
                    )
                    || matches!(code.as_bytes().get(cursor), Some(b'\'' | b'"' | b'`')))
            {
                paths.push(Mention {
                    text: home_relative(token),
                    pattern: false,
                });
            }
        } else {
            if matches!(ch, '\'' | '"' | '`')
                && (cursor == 0 || code.as_bytes()[cursor - 1] != b'\\')
            {
                if quote == 0 {
                    quote = ch as u8;
                } else if quote == ch as u8 {
                    quote = 0;
                }
            }
            cursor += ch.len_utf8();
        }
    }
    paths
}
