use crate::record::{Command, Items, Stdin, Word};

pub(super) struct InputSource {
    pub source: String,
    pub cwd: String,
}

pub(super) fn unescape(text: &str, operand: bool) -> String {
    let text = text.split_once("\\c").map_or(text, |(before, _)| before);
    let bytes = text.as_bytes();
    let mut output = String::new();
    let mut at = 0;
    while at < text.len() {
        if bytes[at] == b'\\'
            && let Some(next) = bytes.get(at + 1)
        {
            let simple = match next {
                b'n' => Some('\n'),
                b't' => Some('\t'),
                b'\\' => Some('\\'),
                _ => None,
            };
            if let Some(character) = simple {
                output.push(character);
                at += 2;
                continue;
            }
            let (start, limit, radix) = match next {
                b'x' => (at + 2, 2, 16),
                b'u' => (at + 2, 4, 16),
                b'U' => (at + 2, 8, 16),
                b'0'..=b'7' => (at + 1, if operand && *next == b'0' { 4 } else { 3 }, 8),
                _ => (at + 1, 0, 8),
            };
            let mut end = start;
            while end < bytes.len() && end - start < limit && (bytes[end] as char).is_digit(radix) {
                end += 1;
            }
            if end > start {
                let value = u64::from_str_radix(&text[start..end], radix).unwrap_or(0);
                output.push(if value == 0 {
                    '\n'
                } else {
                    char::from_u32((value & 65535) as u32).unwrap_or(char::REPLACEMENT_CHARACTER)
                });
                at = end;
                continue;
            }
        }
        let Some(character) = text[at..].chars().next() else {
            break;
        };
        output.push(character);
        at += character.len_utf8();
    }
    output
}

pub(super) fn printf_output(args: &[Word]) -> Option<String> {
    let args = if args.first().is_some_and(|w| w == "--") {
        &args[1..]
    } else {
        args
    };
    let Some(format) = args.first() else {
        return Some(String::new());
    };
    let bytes = format.as_bytes();
    let mut pieces = Vec::new();
    let mut from = 0;
    let mut at = 0;
    while at + 1 < bytes.len() {
        if bytes[at] == b'%' && matches!(bytes[at + 1], b's' | b'b' | b'%') {
            pieces.push(&format[from..at]);
            pieces.push(&format[at..at + 2]);
            at += 2;
            from = at;
        } else {
            at += 1;
        }
    }
    pieces.push(&format[from..]);
    if pieces
        .iter()
        .any(|p| p.contains('%') && !["%s", "%b", "%%"].contains(p))
    {
        return None;
    }
    let converts = pieces.iter().any(|p| ["%s", "%b"].contains(p));
    let mut operands = args[1..].iter();
    let mut output = String::new();
    loop {
        for piece in &pieces {
            match *piece {
                "%%" => output.push('%'),
                "%s" | "%b" => {
                    let value = operands.next().map_or("", Word::as_str);
                    if *piece == "%b" {
                        output.push_str(&unescape(value, true));
                    } else {
                        output.push_str(value);
                    }
                }
                literal => output.push_str(&unescape(literal, false)),
            }
        }
        if !converts || operands.len() == 0 {
            break;
        }
    }
    Some(output)
}

fn name(command: &Command) -> Option<&str> {
    command
        .program
        .and_then(|i| command.argv.get(i))
        .map(|w| w.rsplit('/').next().unwrap_or(w))
}

fn producer(commands: &[Command]) -> Option<(&Command, usize)> {
    commands.iter().find_map(|command| {
        command
            .program
            .filter(|_| matches!(name(command), Some("echo" | "printf")))
            .map(|index| (command, index))
    })
}

pub(super) fn read_input(
    commands: &[Command],
    pipeline: (usize, usize),
    known: impl Fn(&Word) -> bool,
) -> Option<Vec<String>> {
    let stages = commands
        .iter()
        .rev()
        .filter(|command| command.pipeline == Some(pipeline));
    producer_outputs(stages, known)
}

pub(super) fn process_output(commands: &[Command]) -> Option<Vec<String>> {
    producer_outputs(commands.iter().rev(), |word| !word.runtime_unknown)
}

fn producer_outputs<'a>(
    mut stages: impl Iterator<Item = &'a Command>,
    known: impl Fn(&Word) -> bool,
) -> Option<Vec<String>> {
    let last = stages.next()?;
    let spelling = |command: &Command| {
        command
            .argv
            .iter()
            .map(|word| word.raw.clone())
            .collect::<Vec<_>>()
    };
    let key = spelling(last);
    let mut outputs = Vec::new();
    // Only the immediate producer's expanded candidates supply this read.
    // An earlier echo upstream of an unknown program is not its output.
    for command in
        std::iter::once(last).chain(stages.take_while(|command| spelling(command) == key))
    {
        if command.function
            || !command.redirects.is_empty()
            || !command.wrappers.is_empty()
            || !command.environment.is_empty()
        {
            return None;
        }
        let index = command.program?;
        let args = &command.argv[index + 1..];
        if args
            .iter()
            .any(|word| word.expands || word.globs || !known(word))
        {
            return None;
        }
        let values = match name(command) {
            Some("printf") => vec![printf_output(args)?],
            Some("echo") => {
                let (args, newline) = if args.first().is_some_and(|word| word == "-n") {
                    (&args[1..], "")
                } else {
                    (args, "\n")
                };
                if args.first().is_some_and(|word| word.starts_with('-')) {
                    return None;
                }
                let text = args.iter().map(Word::as_str).collect::<Vec<_>>().join(" ");
                // bash's default echo retains escapes; zsh's interprets them.
                vec![
                    format!("{text}{newline}"),
                    format!("{}{newline}", unescape(&text, true)),
                ]
            }
            _ => return None,
        };
        for output in values {
            if !outputs.contains(&output) {
                outputs.push(output);
            }
        }
    }
    Some(outputs)
}

pub(super) fn shell_input(left: &[Command], right: &mut [Command]) -> Vec<InputSource> {
    let Some((producer, index)) = producer(left) else {
        return Vec::new();
    };
    let Some(shell) = right
        .iter_mut()
        .find(|c| super::argv::stdin_kind(c) == Stdin::Shell)
    else {
        return Vec::new();
    };
    let args = &producer.argv[index + 1..];
    if args.iter().any(|word| word.expands || word.runtime_unknown) {
        // Lexical representatives of unknown output are not executable source.
        return Vec::new();
    }
    let source = if name(producer) == Some("printf") {
        let Some(output) = printf_output(args) else {
            return Vec::new();
        };
        output
    } else {
        unescape(
            &args
                .iter()
                .filter(|w| !w.starts_with('-'))
                .map(Word::as_str)
                .collect::<Vec<_>>()
                .join(" "),
            true,
        )
    };
    shell.stdin = Stdin::Shell;
    vec![InputSource {
        source,
        cwd: shell.cwd.clone(),
    }]
}

fn fields(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c| matches!(c, '\u{9}'..='\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'))
        .filter(|s| !s.is_empty())
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
fn strip(text: &str) -> String {
    text.replace(['\'', '"', '\\'], "")
}

pub(super) fn xargs_commands(command: &Command, items: &[String]) -> Vec<InputSource> {
    let Some(index) = command.program else {
        return Vec::new();
    };
    let options = &command.argv[..index];
    let mut marker = "";
    for (i, word) in options.iter().enumerate() {
        if word == "-I" || word == "--replace" {
            marker = options.get(i + 1).map_or("", Word::as_str);
        } else if let Some(value) = word
            .strip_prefix("-I")
            .or_else(|| word.strip_prefix("--replace="))
        {
            marker = value;
        }
    }
    let argv = &command.argv[index..];
    let sources = if marker.is_empty() {
        let mut words = vec![
            argv.iter()
                .map(|w| w.raw.as_str())
                .collect::<Vec<_>>()
                .join(" "),
        ];
        for item in items {
            for value in fields(item) {
                words.extend([quote(value), quote(&strip(value))]);
            }
        }
        vec![words.join(" ")]
    } else {
        let mut sources = Vec::new();
        for item in items {
            for value in [item.clone(), strip(item)] {
                sources.push(
                    argv.iter()
                        .map(|w| {
                            if w.contains(marker) {
                                quote(&w.replace(marker, &value))
                            } else {
                                w.raw.clone()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
        }
        sources
    };
    sources
        .into_iter()
        .map(|source| InputSource {
            source,
            cwd: command.cwd.clone(),
        })
        .collect()
}

pub(super) fn xargs_replacements(left: &[Command], right: &[Command]) -> Vec<InputSource> {
    let Some((producer, index)) = producer(left) else {
        return Vec::new();
    };
    let Some(command) = right
        .iter()
        .find(|c| c.program.is_some() && c.wrappers.iter().any(|w| w == "xargs"))
    else {
        return Vec::new();
    };
    let args = &producer.argv[index + 1..];
    let items: Vec<String> = if name(producer) == Some("printf") {
        let Some(output) = printf_output(args) else {
            return Vec::new();
        };
        output
            .split('\n')
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect()
    } else {
        args.iter()
            .filter(|w| !w.starts_with('-'))
            .map(|w| unescape(w, true))
            .collect()
    };
    xargs_commands(command, &items)
}

pub(super) fn xargs_here_input(command: &Command, body: &str) -> Vec<InputSource> {
    xargs_commands(
        command,
        &fields(body).map(str::to_owned).collect::<Vec<_>>(),
    )
}

fn hidden_name(text: &str) -> bool {
    let name = text.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    name.starts_with('.') && name != "." && name != ".."
}

pub(super) fn mark_walked_input(left: &[Command], right: &mut [Command]) {
    let walker = left.iter().find(|c| {
        let Some(index) = c.program else {
            return false;
        };
        let args = &c.argv[index + 1..];
        name(c) == Some("find")
            || name(c) == Some("fd") && crate::targets::shows_hidden(args)
            || args.iter().any(|w| match name(c) {
                Some("ls") => {
                    ["--all", "--almost-all"].contains(&w.as_str())
                        || w.starts_with('-')
                            && !w.starts_with("--")
                            && w[1..].chars().all(|ch| ch.is_ascii_alphanumeric())
                            && w.contains(['a', 'A'])
                        || !w.starts_with('-') && hidden_name(w)
                }
                Some("echo" | "printf") => !w.starts_with('-') && w.globs && hidden_name(w),
                _ => false,
            })
    });
    if let Some(walker) = walker {
        for command in right {
            if command.wrappers.iter().any(|w| w == "xargs") {
                command.items = Some(Items {
                    root: walker.cwd.clone(),
                    hidden: true,
                });
            }
        }
    }
}
