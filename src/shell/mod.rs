mod brush;
mod cst;
mod divergence;

use crate::{CheckError, CheckErrorKind, CoverageGap, limits::MAX_NESTING};
use std::{
    collections::{BTreeMap, VecDeque},
    ops::Range,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    StructuredOnly,
    Brush,
    TreeSitter,
}

#[derive(Debug, Clone)]
struct Word {
    raw: String,
}
#[derive(Debug, Clone)]
struct Redirect {
    target: Word,
    write: bool,
}
#[derive(Debug, Clone)]
enum Record {
    Definition(String, Vec<Record>),
    Assignment(String, Word),
    LoopBinding(String, Vec<Word>),
    Expansion(Word),
    Command {
        argv: Vec<Word>,
        redirects: Vec<Redirect>,
        pipeline: Option<usize>,
    },
}

struct Parsed {
    records: Option<Vec<Record>>,
    spans: Vec<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRecord {
    pub argv: Vec<String>,
    pub redirects: Vec<(String, bool)>,
    pub unresolved: bool,
    pub pipeline: Option<(usize, usize)>,
    pub cwd: String,
    pub variables: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Observation {
    pub commands: Vec<CommandRecord>,
    pub gaps: Vec<CoverageGap>,
    pub parse_successes: usize,
    pub parse_failures: usize,
    pub executable_qualifier: bool,
}

impl Observation {
    fn gap(&mut self, gap: CoverageGap) {
        if !self.gaps.contains(&gap) {
            self.gaps.push(gap);
        }
    }
}

pub fn check_nesting(source: &str) -> Result<(), CheckError> {
    let mut depth: usize = 0;
    for byte in source.bytes() {
        match byte {
            b'(' | b'{' | b'[' => {
                depth += 1;
                if depth > MAX_NESTING {
                    return Err(CheckError {
                        kind: CheckErrorKind::ResourceLimit,
                    });
                }
            }
            b')' | b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

pub fn observe(
    source: &str,
    arm: Arm,
    home: &str,
    cwd: &str,
    zsh: bool,
) -> Result<Observation, CheckError> {
    let mut variables = BTreeMap::from([
        ("HOME".to_owned(), vec![home.to_owned()]),
        ("PWD".to_owned(), vec![cwd.to_owned()]),
    ]);
    let mut observation = Observation::default();
    observe_source(source, arm, zsh, &mut variables, &mut observation, 0)?;
    Ok(observation)
}

fn observe_source(
    source: &str,
    arm: Arm,
    zsh: bool,
    variables: &mut BTreeMap<String, Vec<String>>,
    output: &mut Observation,
    depth: usize,
) -> Result<(), CheckError> {
    if depth > MAX_NESTING {
        return Err(CheckError {
            kind: CheckErrorKind::ResourceLimit,
        });
    }
    if arm == Arm::StructuredOnly {
        output.gap(CoverageGap::UnsupportedShellSyntax);
        return Ok(());
    }
    let parse = |parsed: &str| match arm {
        Arm::Brush => brush::records(source, parsed),
        Arm::TreeSitter => cst::records(source, parsed),
        Arm::StructuredOnly => unreachable!(),
    };
    let original = parse(source)?;
    let source_id = output.parse_successes + output.parse_failures;
    if original.records.is_some() {
        output.parse_successes += 1;
    } else {
        output.parse_failures += 1;
    }
    let detection = divergence::detect(source, &original.spans)?;
    output.executable_qualifier |= detection.executable_qualifier;
    if detection.divergent {
        output.gap(if zsh {
            CoverageGap::ExecutorDivergence
        } else {
            CoverageGap::UnsupportedDialectConstruct
        });
    }
    let records = if detection.masked != source {
        parse(&detection.masked)?.records
    } else {
        original.records
    };
    let Some(records) = records else {
        if !detection.divergent {
            output.gap(CoverageGap::UnsupportedShellSyntax);
        }
        for code in &detection.code {
            observe_source(code, arm, zsh, &mut variables.clone(), output, depth + 1)?;
        }
        return Ok(());
    };
    let functions: BTreeMap<String, Vec<Record>> = records
        .iter()
        .filter_map(|record| {
            if let Record::Definition(name, body) = record {
                Some((name.clone(), body.clone()))
            } else {
                None
            }
        })
        .collect();
    let mut pending: VecDeque<_> = records.into();
    let mut inspected = 0;
    while let Some(record) = pending.pop_front() {
        inspected += 1;
        if inspected > 512 {
            output.gap(CoverageGap::InspectionBudget);
            break;
        }
        match record {
            Record::Definition(_, body) => {
                // Static preflight retains Go's conservative observation of declared bodies.
                for record in body.iter().rev() {
                    if !matches!(record,Record::Command {argv,..} if argv.first().is_some_and(|w|functions.contains_key(&w.raw)))
                    {
                        pending.push_front(record.clone());
                    }
                }
            }
            Record::Assignment(name, word) => {
                bind(name, &[word], variables, output, arm, zsh, depth)?;
            }
            Record::LoopBinding(name, words) => {
                bind(name, &words, variables, output, arm, zsh, depth)?
            }
            Record::Expansion(word) => {
                for expanded in expand_all(&word.raw, variables, output) {
                    for nested in &expanded.nested {
                        observe_source(
                            nested,
                            arm,
                            zsh,
                            &mut variables.clone(),
                            output,
                            depth + 1,
                        )?;
                    }
                }
            }
            Record::Command {
                argv,
                redirects,
                pipeline,
            } => {
                let mut alternatives = vec![Vec::new()];
                let mut unresolved = false;
                let mut names = Vec::new();
                for word in argv {
                    let mut choices = Vec::new();
                    for expanded in expand_all(&word.raw, variables, output) {
                        for nested in &expanded.nested {
                            observe_source(
                                nested,
                                arm,
                                zsh,
                                &mut variables.clone(),
                                output,
                                depth + 1,
                            )?;
                        }
                        unresolved |= expanded.unresolved;
                        names.extend(expanded.variables);
                        choices.push(expanded.split);
                        choices.push(vec![expanded.unsplit]);
                    }
                    choices.sort();
                    choices.dedup();
                    let mut next = Vec::new();
                    for argv in &alternatives {
                        for choice in &choices {
                            let mut argv = argv.clone();
                            argv.extend(choice.clone());
                            next.push(argv);
                        }
                    }
                    if next.len() > 512 {
                        output.gap(CoverageGap::InspectionBudget);
                        next.truncate(512);
                    }
                    alternatives = next;
                }
                if let Some(body) = alternatives
                    .first()
                    .and_then(|argv| argv.first())
                    .and_then(|name| functions.get(name))
                {
                    for record in body.iter().rev() {
                        pending.push_front(record.clone());
                    }
                    continue;
                }
                let mut targets = Vec::new();
                for redirect in redirects {
                    for expanded in expand_all(&redirect.target.raw, variables, output) {
                        for nested in &expanded.nested {
                            observe_source(
                                nested,
                                arm,
                                zsh,
                                &mut variables.clone(),
                                output,
                                depth + 1,
                            )?;
                        }
                        unresolved |= expanded.unresolved;
                        names.extend(expanded.variables);
                        targets.push((expanded.unsplit, redirect.write));
                    }
                }
                // Assign target roles to each complete argv; never flatten operands before roles.
                let cwds = variables.get("PWD").cloned().unwrap_or_default();
                for argv in alternatives {
                    for cwd in &cwds {
                        output.commands.push(CommandRecord {
                            argv: argv.clone(),
                            redirects: targets.clone(),
                            unresolved,
                            pipeline: pipeline.map(|id| (source_id, id)),
                            cwd: cwd.clone(),
                            variables: names.clone(),
                        });
                        if argv.first().is_some_and(|s| s == "cd")
                            && let Some(target) = argv.iter().skip(1).find(|s| !s.starts_with('-'))
                        {
                            let next =
                                crate::filesystem::normalize(target, cwd, &variables["HOME"][0]);
                            let dirs = variables.entry("PWD".into()).or_default();
                            if !dirs.contains(&next) {
                                dirs.push(next);
                            }
                        }
                    }
                }
                if output.commands.len() > 512 {
                    output.gap(CoverageGap::InspectionBudget);
                    break;
                }
            }
        }
    }
    for name in &detection.evaluated_variables {
        if let Some(codes) = variables.get(name).cloned() {
            for code in codes {
                observe_source(&code, arm, zsh, &mut variables.clone(), output, depth + 1)?;
            }
        }
    }
    for code in &detection.code {
        observe_source(code, arm, zsh, &mut variables.clone(), output, depth + 1)?;
    }
    Ok(())
}

fn bind(
    name: String,
    words: &[Word],
    variables: &mut BTreeMap<String, Vec<String>>,
    output: &mut Observation,
    arm: Arm,
    zsh: bool,
    depth: usize,
) -> Result<(), CheckError> {
    let mut values = Vec::new();
    for word in words {
        for expanded in expand_all(&word.raw, variables, output) {
            for code in &expanded.nested {
                observe_source(code, arm, zsh, &mut variables.clone(), output, depth + 1)?;
            }
            if !values.contains(&expanded.unsplit) {
                values.push(expanded.unsplit);
            }
        }
    }
    if values.len() > 512 {
        output.gap(CoverageGap::InspectionBudget);
        values.truncate(512);
    }
    variables.insert(name, values);
    Ok(())
}

fn expand_all(
    raw: &str,
    variables: &BTreeMap<String, Vec<String>>,
    output: &mut Observation,
) -> Vec<Expanded> {
    let mut contexts = vec![BTreeMap::new()];
    for (name, values) in variables {
        if !raw.contains(&format!("${name}")) && !raw.contains(&format!("${{{name}")) {
            continue;
        }
        let mut next = Vec::new();
        'combinations: for context in &contexts {
            for value in values {
                if next.len() == 512 {
                    output.gap(CoverageGap::InspectionBudget);
                    break 'combinations;
                }
                let mut context = context.clone();
                context.insert(name.clone(), value.clone());
                next.push(context);
            }
        }
        contexts = next;
    }
    contexts
        .iter()
        .map(|context| expand(raw, context))
        .collect()
}

struct Expanded {
    split: Vec<String>,
    unsplit: String,
    unresolved: bool,
    nested: Vec<String>,
    variables: Vec<String>,
}

fn expand(raw: &str, variables: &BTreeMap<String, String>) -> Expanded {
    let mut output = String::new();
    let mut nested = Vec::new();
    let mut names = Vec::new();
    let mut unresolved = false;
    let mut quote = 0;
    let mut split_points = false;
    let mut cursor = 0;
    while cursor < raw.len() {
        let tail = &raw[cursor..];
        let byte = raw.as_bytes()[cursor];
        if matches!(byte, b'\'' | b'"') {
            if quote == 0 {
                quote = byte;
                cursor += 1;
                continue;
            }
            if quote == byte {
                quote = 0;
                cursor += 1;
                continue;
            }
        }
        if byte == b'\\' && quote != b'\'' {
            cursor += 1;
            if cursor < raw.len() {
                let ch = raw[cursor..].chars().next().unwrap_or_default();
                if ch != '\n' {
                    if quote == b'"' && !matches!(ch, '$' | '`' | '"' | '\\') {
                        output.push('\\');
                    }
                    output.push(ch);
                }
                cursor += ch.len_utf8();
            }
            continue;
        }
        if byte == b'`'
            && quote != b'\''
            && let Some(length) = raw[cursor + 1..].find('`')
        {
            nested.push(raw[cursor + 1..cursor + 1 + length].to_owned());
            unresolved = true;
            output.push_str("__observed_stream__");
            cursor += length + 2;
            continue;
        }
        if quote != b'\''
            && (tail.starts_with("$(") || tail.starts_with("<(") || tail.starts_with(">("))
            && let Some(end) = divergence::closing(raw, cursor + 1, b'(', b')')
        {
            nested.push(raw[cursor + 2..end].to_owned());
            unresolved |= tail.starts_with("$(");
            output.push_str("__observed_stream__");
            cursor = end + 1;
            continue;
        }
        if quote != b'\'' && byte == b'$' {
            let braced = tail.starts_with("${");
            let start = cursor + if braced { 2 } else { 1 };
            let length = raw[start..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .map(char::len_utf8)
                .sum::<usize>();
            if length > 0 {
                let name = &raw[start..start + length];
                names.push(name.to_owned());
                if let Some(value) = variables.get(name) {
                    output.push_str(value);
                    if quote == 0 {
                        split_points = true;
                    }
                } else {
                    unresolved = true;
                }
                cursor = start
                    + length
                    + usize::from(braced && raw.as_bytes().get(start + length) == Some(&b'}'));
                continue;
            }
        }
        let ch = tail.chars().next().unwrap_or_default();
        output.push(ch);
        cursor += ch.len_utf8();
    }
    let split = if split_points {
        output.split_whitespace().map(str::to_owned).collect()
    } else {
        vec![output.clone()]
    };
    Expanded {
        split,
        unsplit: output,
        unresolved,
        nested,
        variables: names,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_alternative_argv() {
        for arm in [Arm::Brush, Arm::TreeSitter] {
            let obs = observe("p='public protected'; cat $p", arm, "/h", "/h/p", true).unwrap();
            assert!(
                obs.commands
                    .iter()
                    .any(|c| c.argv == ["cat", "public", "protected"])
            );
            assert!(
                obs.commands
                    .iter()
                    .any(|c| c.argv == ["cat", "public protected"])
            );
        }
    }
}
