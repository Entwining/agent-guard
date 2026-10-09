use crate::record::{Effect, HostFacts, Role, Target, Walk, Word};
use std::rc::Rc;

pub(super) fn infer(program: &str, words: &[Word], cwd: &str, host: HostFacts<'_>) -> Vec<Target> {
    let mut operands = Vec::new();
    let mut directory = None;
    let mut symbolic = false;
    let mut options = true;
    let mut index = 0;
    while let Some(word) = words.get(index) {
        index += 1;
        if options && word == "--" {
            options = false;
            continue;
        }
        if options && word.role != Role::Path && word.starts_with('-') {
            if program == "ln" && word == "--symbolic" {
                symbolic = true;
            }
            let value_option = if let Some(long) = word.strip_prefix("--") {
                let (key, value) = long.split_once('=').unwrap_or((long, ""));
                ["target-directory", "suffix"]
                    .contains(&key)
                    .then_some((key == "target-directory", value))
            } else {
                let value_option = word
                    .char_indices()
                    .skip(1)
                    .find(|(_, letter)| matches!(letter, 't' | 'S'));
                if program == "ln" {
                    let flags_end = value_option.map_or(word.len(), |(at, _)| at);
                    symbolic |= word[..flags_end].contains('s');
                }
                value_option.map(|(at, letter)| (letter == 't', &word[at + 1..]))
            };
            if program != "rm"
                && let Some((is_directory, value)) = value_option
            {
                let value = if value.is_empty() {
                    let value = words.get(index).cloned();
                    index += 1;
                    value
                } else {
                    Some(word.with_text(value.into()))
                };
                if is_directory {
                    directory = value;
                }
            }
            continue;
        }
        operands.push(word.clone());
    }
    let destination = if program == "rm" {
        None
    } else if directory.is_some() {
        directory
    } else if program == "ln" && operands.len() == 1 {
        Some(Word::literal(".".into()))
    } else {
        operands.pop()
    };
    let destination = destination.map(|word| {
        Rc::new(Target::from_word(
            &word,
            cwd,
            host,
            Effect::Change,
            Walk::None,
        ))
    });
    let relocate = program == "mv" || program == "ln" && !symbolic;
    let walk = if program == "mv" {
        Walk::None
    } else {
        Walk::Visible
    };
    let mut targets: Vec<_> = operands
        .iter()
        .map(|word| {
            let mut target = Target::from_word(word, cwd, host, Effect::Meta, walk);
            if relocate && let Some(destination) = &destination {
                target.effect = Effect::Change;
                target.relocation_destination = Some(Rc::clone(destination));
            }
            target
        })
        .collect();
    if let Some(destination) = destination {
        let mut target = (*destination).clone();
        target.effect = Effect::Meta;
        target.walk = walk;
        targets.push(target);
    }
    targets
}
