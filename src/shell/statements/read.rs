use super::super as shell;
use super::*;

struct ReadOptions {
    names: Vec<String>,
    raw: bool,
    delimiter: char,
    count: Option<usize>,
    input_fd: usize,
    modeled: bool,
    rejected: bool,
    array_name: Option<String>,
    default_reply: bool,
}

impl ReadOptions {
    fn parse(args: &[crate::record::Word]) -> Self {
        let mut names = Vec::new();
        let mut index = 0;
        let mut raw = false;
        let mut delimiter = '\n';
        let mut count = None;
        let mut input_fd = 0;
        let mut modeled = true;
        let mut rejected = false;
        let mut array_name = None;
        while let Some(word) = args.get(index) {
            if word == "--" {
                index += 1;
                break;
            }
            let Some(flags) = word.strip_prefix('-').filter(|flags| !flags.is_empty()) else {
                break;
            };
            for (at, option) in flags.char_indices() {
                match option {
                    'r' => raw = true,
                    's' => {}
                    'd' | 'n' | 't' | 'u' | 'a' | 'p' => {
                        let attached = &flags[at + option.len_utf8()..];
                        let value = if attached.is_empty() {
                            index += 1;
                            args.get(index).map(|word| {
                                modeled &= !word.expands;
                                word.text.as_str()
                            })
                        } else {
                            modeled &= !word.expands;
                            Some(attached)
                        };
                        let Some(value) = value else {
                            modeled = false;
                            break;
                        };
                        match option {
                            'd' => delimiter = value.chars().next().unwrap_or('\0'),
                            'n' => match value.parse::<usize>() {
                                Ok(value) => count = Some(value),
                                Err(_) => modeled = false,
                            },
                            't' => modeled &= value.parse::<f64>().is_ok_and(|value| value > 0.0),
                            'u' => match value.parse::<usize>() {
                                Ok(value) => input_fd = value,
                                Err(_) => modeled = false,
                            },
                            'a' => {
                                if identifier(value) {
                                    array_name = Some(value.to_owned());
                                    names.push(value.to_owned());
                                } else {
                                    modeled = false;
                                }
                            }
                            'p' => rejected = true,
                            _ => unreachable!(),
                        }
                        break;
                    }
                    _ => modeled = false,
                }
            }
            index += 1;
        }
        for word in &args[index.min(args.len())..] {
            if identifier(&word.text) {
                names.push(word.text.clone());
            } else {
                modeled = false;
            }
        }
        let default_reply = names.is_empty() && array_name.is_none() && index == args.len();
        if default_reply {
            names.push("REPLY".into());
        }
        Self {
            names,
            raw,
            delimiter,
            count,
            input_fd,
            modeled,
            rejected,
            array_name,
            default_reply,
        }
    }
}

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn read(
        &mut self,
        args: &[crate::record::Word],
        scope: &mut Scope,
    ) -> Result<(), CheckError> {
        let ReadOptions {
            names,
            raw,
            delimiter,
            count,
            input_fd,
            mut modeled,
            mut rejected,
            array_name,
            default_reply,
        } = ReadOptions::parse(args);
        let source = scope.input(input_fd as i32);
        let candidates = source
            .map(|input| self.flow.candidates(input))
            .transpose()?;
        let unknown = candidates
            .as_ref()
            .is_none_or(|values| values.iter().any(|value| value.unknown));
        let known = read_known_candidates(candidates.as_deref(), delimiter);
        let input = known.as_ref().map(|values| {
            values
                .iter()
                .map(|value| value.text.clone())
                .collect::<Vec<_>>()
        });
        modeled &= !names.is_empty()
            && (default_reply || array_name.is_some() || !scope.bindings.contains_key("IFS"))
            && (raw
                || input
                    .as_ref()
                    .is_none_or(|values| values.iter().all(|value| !value.contains('\\'))));
        if array_name.is_some()
            && input.as_ref().is_some_and(|values| {
                values.iter().any(|value| {
                    !matches!(
                        shell::arithmetic::armed(value),
                        shell::arithmetic::Arming::Inert
                    )
                })
            })
        {
            rejected = true;
        }
        let known_input = modeled && !rejected;
        if known_input && let Some(candidates) = &candidates {
            self.read_progress(scope, candidates, delimiter, count, input_fd);
        }
        if let Some(name) = array_name.filter(|_| !rejected) {
            self.read_array_input(
                &name,
                input.as_deref(),
                scope,
                (known_input, unknown, count, delimiter),
            );
            return Ok(());
        }
        let fields = read_fields(
            input.as_deref().filter(|_| known_input),
            delimiter,
            count,
            default_reply,
            names.len(),
        );
        self.read_bindings(
            names,
            fields.as_deref(),
            known.as_deref(),
            scope,
            (unknown, count, rejected, !modeled || input_fd != 0),
        );
        if !known_input
            && input.as_ref().is_some_and(|values| {
                values.iter().any(|value| {
                    !matches!(
                        shell::arithmetic::armed(&value.replace('\\', "")),
                        shell::arithmetic::Arming::Inert
                    )
                })
            })
        {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
        }
        Ok(())
    }

    fn read_progress(
        &mut self,
        scope: &mut Scope,
        candidates: &[crate::record::stream::Candidate],
        delimiter: char,
        count: Option<usize>,
        input_fd: usize,
    ) {
        let mut remaining = Vec::new();
        for candidate in candidates {
            let value = &candidate.text;
            let boundary = value.find(delimiter).unwrap_or(value.len());
            let prefix = &value[..boundary];
            let consumed =
                if let Some(count) = count.filter(|count| *count <= prefix.chars().count()) {
                    value
                        .char_indices()
                        .nth(count)
                        .map_or(value.len(), |(at, _)| at)
                } else {
                    boundary
                        + if boundary < value.len() {
                            delimiter.len_utf8()
                        } else {
                            0
                        }
                };
            let mut rest = candidate.clone();
            rest.text = value[consumed..].to_owned();
            rest.cuts = candidate
                .cuts
                .iter()
                .filter(|cut| **cut >= consumed)
                .map(|cut| cut - consumed)
                .collect();
            remaining.push(rest);
            for cut in candidate
                .cuts
                .iter()
                .copied()
                .filter(|cut| *cut <= boundary)
            {
                let mut rest = candidate.clone();
                rest.text = value[cut..].into();
                rest.cuts = candidate
                    .cuts
                    .iter()
                    .filter(|at| **at >= cut)
                    .map(|at| at - cut)
                    .collect();
                remaining.push(rest);
            }
            if count.is_some() {
                remaining.push(candidate.clone());
            }
        }
        let rest = self.flow.materialize(remaining);
        scope.advance_input(input_fd as i32, rest);
    }

    fn read_bindings(
        &mut self,
        names: Vec<String>,
        fields: Option<&[Vec<String>]>,
        known: Option<&[crate::record::stream::Candidate]>,
        scope: &mut Scope,
        context: (bool, Option<usize>, bool, bool),
    ) {
        let (unknown, count, rejected, preserve_prior) = context;
        for (position, name) in names.into_iter().enumerate() {
            let mut values = Vec::new();
            if let Some(fields) = fields {
                for candidate in fields {
                    let value = BindingValue::Known(candidate[position].clone());
                    if !values.contains(&value) {
                        values.push(value);
                    }
                }
            }
            if fields.is_none() || unknown {
                values.push(BindingValue::RuntimeUnknown(None));
            }
            // zsh rejects -a/-p, and -n writes an empty value rather than bash's prefix.
            if count.is_some() && !values.contains(&BindingValue::Known(String::new())) {
                values.push(BindingValue::Known(String::new()));
            }
            if (rejected || preserve_prior)
                && let Some(prior) = scope.bindings.get(&name)
            {
                for value in prior.values.iter() {
                    if !values.contains(value) {
                        values.push(value.clone());
                    }
                }
            }
            scope.assign(name.clone(), values);
            if let (Some(fields), Some(known)) = (fields, known) {
                let mut origins = Origins::new();
                for (field, candidate) in fields.iter().zip(known) {
                    origins
                        .entry(field[position].clone())
                        .or_default()
                        .push(candidate.guard.clone());
                }
                scope.set_origins(&name, origins);
            }
        }
    }
    fn read_array_input(
        &mut self,
        name: &str,
        input: Option<&[String]>,
        scope: &mut Scope,
        context: (bool, bool, Option<usize>, char),
    ) {
        let (known_input, unknown, count, delimiter) = context;
        let values = input.filter(|_| known_input).map(|values| {
            values
                .iter()
                .map(|value| {
                    value
                        .split(delimiter)
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(count.unwrap_or(usize::MAX))
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
        });
        self.read_array_fields(name, values.as_deref(), unknown, scope);
        if !known_input
            && input.is_some_and(|values| {
                values.iter().any(|value| {
                    !matches!(
                        shell::arithmetic::armed(value),
                        shell::arithmetic::Arming::Inert
                    )
                })
            })
        {
            self.output.gap(CoverageGap::UnsupportedShellSyntax);
        }
    }
}

fn read_fields(
    input: Option<&[String]>,
    delimiter: char,
    count: Option<usize>,
    default_reply: bool,
    names: usize,
) -> Option<Vec<Vec<String>>> {
    input.map(|values| {
        values
            .iter()
            .map(|value| {
                let data = value
                    .split(delimiter)
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(count.unwrap_or(usize::MAX))
                    .collect::<String>();
                if default_reply {
                    return vec![data];
                }
                let mut remainder = data.as_str();
                (0..names)
                    .map(|position| {
                        remainder = remainder.trim_start_matches([' ', '\t', '\n']);
                        let (field, rest) = if position + 1 == names {
                            (remainder.trim_end_matches([' ', '\t', '\n']), "")
                        } else {
                            let end = remainder.find([' ', '\t', '\n']).unwrap_or(remainder.len());
                            (&remainder[..end], &remainder[end..])
                        };
                        remainder = rest;
                        field.to_owned()
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    })
}

fn read_known_candidates(
    candidates: Option<&[crate::record::stream::Candidate]>,
    delimiter: char,
) -> Option<Vec<crate::record::stream::Candidate>> {
    candidates.map(|values| {
        let mut known = Vec::new();
        for value in values.iter().filter(|value| value.known) {
            known.push(value.clone());
            let boundary = value.text.find(delimiter).unwrap_or(value.text.len());
            for cut in value.cuts.iter().copied().filter(|cut| *cut <= boundary) {
                let mut prefix = value.clone();
                prefix.text = value.text[..cut].into();
                prefix.cuts.retain(|at| *at <= cut);
                known.push(prefix);
            }
        }
        known
    })
}
