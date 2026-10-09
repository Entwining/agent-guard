use super::*;

pub(super) fn infer_tar(
    args: &[Word],
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) -> Vec<usize> {
    let mut claimed = Vec::new();
    let mut archive = None;
    let mut create = false;
    let mut extract = false;
    let mut stdout = false;
    let mut operands = Vec::new();
    let mut base = cwd.to_owned();
    let mut index = 0;
    let mut options = true;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" && options {
            options = false;
            index += 1;
            continue;
        }
        if !options {
            operands.push(arg.clone());
            claimed.push(index);
            index += 1;
            continue;
        }
        if arg.role == Role::Option(OptionRole::Name) {
            index += 1;
            continue;
        }
        let cluster = !arg.starts_with("--") && (index == 0 || arg.starts_with('-'));
        let flags = arg.strip_prefix('-').unwrap_or(arg.as_str());
        let short = cluster
            .then(|| flags.char_indices().find(|(_, c)| "fCTX".contains(*c)))
            .flatten();
        let letters = short.map_or(flags, |(at, c)| &flags[..at + c.len_utf8()]);
        if cluster {
            create |= letters.contains('c');
            extract |= letters.contains('x');
            stdout |= letters.contains('O');
        }
        create |= arg == "--create";
        extract |= ["--extract", "--get"].contains(&arg.as_str());
        stdout |= arg == "--to-stdout";
        if let Some((at, option)) = short {
            claimed.push(index);
            let tail = &flags[at + option.len_utf8()..];
            let value = if tail.is_empty() {
                index += 1;
                claimed.push(index);
                args.get(index).cloned()
            } else {
                Some(arg.with_text(tail.into()))
            };
            if let Some(value) = value {
                match option {
                    'f' => archive = Some(value),
                    'C' => base = crate::filesystem::normalize(&value, &base, ""),
                    _ => {
                        let mut target =
                            Target::from_word(&value, cwd, host, Effect::Read, Walk::Visible);
                        target.via = Via::Option;
                        effects.targets.push(target);
                    }
                }
            }
        } else if arg == "--file" {
            index += 1;
            archive = args.get(index).cloned();
            claimed.push(index);
        } else if let Some(path) = arg.strip_prefix("--file=") {
            archive = Some(arg.with_text(path.to_owned()));
            claimed.push(index);
        } else if let Some(path) = arg.strip_prefix("--directory=") {
            base = crate::filesystem::normalize(path, &base, host.home);
            claimed.push(index);
        } else if arg == "--directory" || arg == "--cd" {
            index += 1;
            claimed.push(index);
            if let Some(path) = args.get(index) {
                base = crate::filesystem::normalize(path, &base, "");
            }
        } else if !cluster && !arg.starts_with('-') {
            operands.push(arg.clone());
            claimed.push(index);
        }
        index += 1;
    }
    archive_target(archive.as_ref(), create, cwd, host, effects);
    effects.independent_arguments = independent_operands(args, &operands);
    operand_targets(&operands, &base, cwd, host, effects);
    if extract && !stdout {
        effects
            .targets
            .push(Target::new(base, Effect::Write, Walk::None, Via::Operand));
    }
    claimed
}

fn operand_targets(
    operands: &[Word],
    base: &str,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    for path in operands {
        effects.targets.push(Target::from_word(
            &path.with_text(if path.starts_with('~') {
                path.text.clone()
            } else {
                crate::filesystem::normalize(path, base, "")
            }),
            cwd,
            host,
            Effect::Read,
            Walk::Visible,
        ));
    }
}

fn archive_target(
    archive: Option<&Word>,
    create: bool,
    cwd: &str,
    host: HostFacts<'_>,
    effects: &mut Effects,
) {
    if let Some(path) = archive {
        effects.targets.push(Target::from_word(
            path,
            cwd,
            host,
            if create { Effect::Write } else { Effect::Read },
            Walk::None,
        ));
    }
}
