use super::{Expanded, ExpansionContext, ParameterExpr, parameter_name};
use crate::CheckError;
use brush_parser::word::{ParameterTestType, SubstringMatchKind};

pub(super) struct Fragment {
    pub value: Option<String>,
    pub pattern: String,
    pub assignments: Vec<(String, String)>,
}

pub(super) fn value(
    expr: &ParameterExpr,
    context: &ExpansionContext<'_>,
    fragments: &[(&str, Fragment)],
    out: &mut Expanded,
) -> Result<Option<String>, CheckError> {
    use ParameterExpr::*;
    let Some(mut name) = parameter_name(expr) else {
        return Ok(None);
    };
    let indirect = match expr {
        Parameter { indirect, .. }
        | UseDefaultValues { indirect, .. }
        | AssignDefaultValues { indirect, .. }
        | UseAlternativeValue { indirect, .. }
        | ParameterLength { indirect, .. }
        | Substring { indirect, .. }
        | RemoveSmallestPrefixPattern { indirect, .. }
        | RemoveLargestPrefixPattern { indirect, .. }
        | RemoveSmallestSuffixPattern { indirect, .. }
        | RemoveLargestSuffixPattern { indirect, .. }
        | ReplaceSubstring { indirect, .. }
        | UppercaseFirstChar { indirect, .. }
        | UppercasePattern { indirect, .. }
        | LowercaseFirstChar { indirect, .. }
        | LowercasePattern { indirect, .. } => *indirect,
        _ => return Ok(None),
    };
    let lookup = |name: &str| {
        out.assignments
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
            .or_else(|| context.get(name).cloned())
            .or_else(|| (name == "PWD").then(|| context.cwd.into()))
    };
    if context.unknown_variables.contains(&name) {
        return Ok(None);
    }
    if indirect {
        let Some(value) = lookup(&name) else {
            return Ok(None);
        };
        name = value;
        if context.unknown_variables.contains(&name) {
            return Ok(None);
        }
    }
    let current = lookup(&name);
    if indirect && !out.word.vars.contains(&name) {
        out.word.vars.push(name.clone());
    }
    let get = |raw: &str| {
        fragments
            .iter()
            .find(|(key, _)| *key == raw)
            .map(|(_, value)| value)
    };
    let body = |raw: Option<&str>| raw.map_or(Some(String::new()), |raw| get(raw)?.value.clone());
    let pattern = |raw: Option<&str>| {
        raw.map_or(Some(String::new()), |raw| {
            let fragment = get(raw)?;
            fragment.value.as_ref()?;
            Some(fragment.pattern.clone())
        })
    };
    let present = |test: &ParameterTestType| {
        current
            .as_ref()
            .is_some_and(|value| matches!(test, ParameterTestType::Unset) || !value.is_empty())
    };
    let result = match expr {
        Parameter { .. } => current,
        UseDefaultValues {
            test_type,
            default_value,
            ..
        }
        | AssignDefaultValues {
            test_type,
            default_value,
            ..
        } => {
            if present(test_type) {
                current
            } else {
                let value = body(default_value.as_deref());
                if let Some(value) = &value {
                    if let Some(fragment) = default_value.as_deref().and_then(get) {
                        out.assignments.extend(fragment.assignments.clone());
                    }
                    if matches!(expr, AssignDefaultValues { .. }) {
                        out.assignments.push((name, value.clone()));
                    }
                }
                value
            }
        }
        UseAlternativeValue {
            test_type,
            alternative_value,
            ..
        } => {
            if present(test_type) {
                let value = body(alternative_value.as_deref());
                if value.is_some()
                    && let Some(fragment) = alternative_value.as_deref().and_then(get)
                {
                    out.assignments.extend(fragment.assignments.clone());
                }
                value
            } else {
                Some(String::new())
            }
        }
        ParameterLength { .. } => current.map(|value| value.chars().count().to_string()),
        RemoveSmallestPrefixPattern { pattern: raw, .. }
        | RemoveLargestPrefixPattern { pattern: raw, .. }
        | RemoveSmallestSuffixPattern { pattern: raw, .. }
        | RemoveLargestSuffixPattern { pattern: raw, .. } => {
            let (Some(value), Some(pattern)) = (current, pattern(raw.as_deref())) else {
                return Ok(None);
            };
            let prefix = matches!(
                expr,
                RemoveSmallestPrefixPattern { .. } | RemoveLargestPrefixPattern { .. }
            );
            let largest = matches!(
                expr,
                RemoveLargestPrefixPattern { .. } | RemoveLargestSuffixPattern { .. }
            );
            let mut boundaries: Vec<_> = value
                .char_indices()
                .map(|(at, _)| at)
                .chain(std::iter::once(value.len()))
                .collect();
            if prefix == largest {
                boundaries.reverse();
            }
            let mut result = value.clone();
            for at in boundaries {
                crate::check_deadline(context.deadline)?;
                if crate::filesystem::parameter_pattern_matches(
                    &pattern,
                    if prefix { &value[..at] } else { &value[at..] },
                ) {
                    result = if prefix {
                        value[at..].into()
                    } else {
                        value[..at].into()
                    };
                    break;
                }
            }
            Some(result)
        }
        Substring { offset, length, .. } => {
            let numeric = |raw: &str| {
                let value = get(raw)
                    .and_then(|fragment| fragment.value.as_deref())
                    .unwrap_or(raw);
                super::super::arithmetic::integer(
                    value,
                    context.variables,
                    context.unknown_variables,
                    context.deadline,
                )
                .map(|integer| integer.value)
            };
            let (Some(value), Some(offset)) = (current, numeric(&offset.value)?) else {
                return Ok(None);
            };
            let chars: Vec<_> = value.chars().collect();
            let start = if offset < 0 {
                (chars.len() as i64 + offset).max(0)
            } else {
                offset
            } as usize;
            let end = match length {
                None => Some(chars.len()),
                Some(length) => numeric(&length.value)?.map(|n| {
                    if n < 0 {
                        (chars.len() as i64 + n).max(0) as usize
                    } else {
                        start.saturating_add(n as usize)
                    }
                }),
            };
            end.map(|end| {
                chars[start.min(chars.len())..end.max(start).min(chars.len())]
                    .iter()
                    .collect()
            })
        }
        ReplaceSubstring {
            pattern: raw,
            replacement,
            match_kind,
            ..
        } => {
            let (Some(value), Some(pattern), Some(replacement)) =
                (current, pattern(Some(raw)), body(replacement.as_deref()))
            else {
                return Ok(None);
            };
            let boundaries: Vec<_> = value
                .char_indices()
                .map(|(at, _)| at)
                .chain(std::iter::once(value.len()))
                .collect();
            let mut result = String::new();
            let mut cursor = 0;
            let mut replaced = false;
            for (position, &start) in boundaries.iter().enumerate() {
                if start < cursor
                    || replaced && !matches!(match_kind, SubstringMatchKind::Anywhere)
                    || matches!(match_kind, SubstringMatchKind::Prefix) && start != 0
                {
                    continue;
                }
                let mut found = None;
                for &end in boundaries[position..].iter().rev() {
                    crate::check_deadline(context.deadline)?;
                    if matches!(match_kind, SubstringMatchKind::Suffix) && end != value.len() {
                        continue;
                    }
                    if crate::filesystem::parameter_pattern_matches(&pattern, &value[start..end]) {
                        found = Some(end);
                        break;
                    }
                }
                if let Some(end) = found {
                    result.push_str(&value[cursor..start]);
                    result.push_str(&replacement);
                    cursor = end;
                    replaced = true;
                    if start == end && end < value.len() {
                        let next = boundaries[position + 1];
                        result.push_str(&value[end..next]);
                        cursor = next;
                    }
                }
            }
            result.push_str(&value[cursor..]);
            Some(result)
        }
        UppercaseFirstChar { pattern: raw, .. }
        | UppercasePattern { pattern: raw, .. }
        | LowercaseFirstChar { pattern: raw, .. }
        | LowercasePattern { pattern: raw, .. } => {
            let (Some(value), Some(pattern)) = (current, pattern(raw.as_deref())) else {
                return Ok(None);
            };
            let pattern = if pattern.is_empty() { "?" } else { &pattern };
            let first = matches!(expr, UppercaseFirstChar { .. } | LowercaseFirstChar { .. });
            let upper = matches!(expr, UppercaseFirstChar { .. } | UppercasePattern { .. });
            let mut result = String::new();
            for (at, ch) in value.chars().enumerate() {
                if (!first || at == 0)
                    && crate::filesystem::parameter_pattern_matches(pattern, &ch.to_string())
                {
                    if upper {
                        result.extend(ch.to_uppercase());
                    } else {
                        result.extend(ch.to_lowercase());
                    }
                } else {
                    result.push(ch);
                }
            }
            Some(result)
        }
        _ => None,
    };
    if result.is_some()
        && matches!(
            expr,
            RemoveSmallestPrefixPattern { .. }
                | RemoveLargestPrefixPattern { .. }
                | RemoveSmallestSuffixPattern { .. }
                | RemoveLargestSuffixPattern { .. }
                | ReplaceSubstring { .. }
                | Substring { .. }
                | UppercaseFirstChar { .. }
                | UppercasePattern { .. }
                | LowercaseFirstChar { .. }
                | LowercasePattern { .. }
        )
    {
        for (_, fragment) in fragments {
            out.assignments.extend(fragment.assignments.clone());
        }
    }
    Ok(result)
}
