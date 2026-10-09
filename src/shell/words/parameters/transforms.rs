use super::*;

pub(super) fn remove_pattern(
    expr: &ParameterExpr,
    current: Option<String>,
    pattern: Option<String>,
    context: &ExpansionContext<'_>,
) -> Result<Option<String>, CheckError> {
    use ParameterExpr::*;
    let (Some(value), Some(pattern)) = (current, pattern) else {
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
    Ok(Some(result))
}

pub(super) fn substring(
    current: Option<String>,
    offset: &brush_parser::ast::UnexpandedArithmeticExpr,
    length: &Option<brush_parser::ast::UnexpandedArithmeticExpr>,
    context: &ExpansionContext<'_>,
    fragments: &[(&str, Fragment)],
) -> Result<Option<String>, CheckError> {
    let get = |raw: &str| {
        fragments
            .iter()
            .find(|(key, _)| *key == raw)
            .map(|(_, value)| value)
    };
    let numeric = |raw: &str| {
        let value = get(raw)
            .and_then(|fragment| fragment.value.as_deref())
            .unwrap_or(raw);
        crate::shell::arithmetic::integer(
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
    Ok(end.map(|end| {
        chars[start.min(chars.len())..end.max(start).min(chars.len())]
            .iter()
            .collect()
    }))
}

pub(super) fn replace_pattern(
    current: Option<String>,
    pattern: Option<String>,
    replacement: Option<String>,
    match_kind: &SubstringMatchKind,
    context: &ExpansionContext<'_>,
) -> Result<Option<String>, CheckError> {
    let (Some(value), Some(pattern), Some(replacement)) = (current, pattern, replacement) else {
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
    Ok(Some(result))
}

pub(super) fn change_case(
    expr: &ParameterExpr,
    current: Option<String>,
    pattern: Option<String>,
) -> Option<String> {
    use ParameterExpr::*;
    let (Some(value), Some(pattern)) = (current, pattern) else {
        return None;
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
