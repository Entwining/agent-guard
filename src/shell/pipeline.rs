use crate::record::{Command, Stdin, Word};

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
