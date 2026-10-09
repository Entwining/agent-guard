use super::{Expanded, ExpansionContext, ParameterExpr, parameter_name};
use crate::CheckError;
use brush_parser::word::{ParameterTestType, SubstringMatchKind};

pub(super) struct Fragment {
    pub value: Option<String>,
    pub pattern: String,
    pub assignments: Vec<(String, String)>,
    pub has_variables: bool,
}

pub(super) fn needs_empty_context(
    expr: &ParameterExpr,
    fragments: &[(&str, Fragment)],
    nested: bool,
) -> bool {
    match expr {
        ParameterExpr::UseDefaultValues {
            test_type,
            default_value,
            indirect: false,
            ..
        }
        | ParameterExpr::AssignDefaultValues {
            test_type,
            default_value,
            indirect: false,
            ..
        } if !matches!(test_type, ParameterTestType::Unset) && !nested => {
            default_value.as_deref().is_none_or(|raw| {
                fragments
                    .iter()
                    .find(|(key, _)| *key == raw)
                    .is_none_or(|(_, fragment)| {
                        fragment.has_variables
                            || fragment.value.as_ref().is_none_or(|value| {
                                value.is_empty() || value.chars().any(char::is_whitespace)
                            })
                    })
            })
        }
        ParameterExpr::UseDefaultValues { .. }
        | ParameterExpr::AssignDefaultValues { .. }
        | ParameterExpr::UseAlternativeValue { .. } => true,
        _ => false,
    }
}

fn apply_default(
    expr: &ParameterExpr,
    name: String,
    raw: Option<&str>,
    fragments: &[(&str, Fragment)],
    out: &mut Expanded,
) -> Option<String> {
    let fragment = raw.and_then(|raw| fragments.iter().find(|(key, _)| *key == raw));
    let value = if raw.is_none() {
        Some(String::new())
    } else {
        fragment?.1.value.clone()
    };
    if let Some(value) = &value {
        if let Some((_, fragment)) = fragment {
            out.assignments.extend(fragment.assignments.clone());
        }
        if matches!(expr, ParameterExpr::AssignDefaultValues { .. }) {
            out.assignments.push((name, value.clone()));
        }
    }
    value
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
    let Some(indirect) = indirect(expr) else {
        return Ok(None);
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
        if let UseDefaultValues {
            test_type,
            default_value,
            indirect: false,
            ..
        }
        | AssignDefaultValues {
            test_type,
            default_value,
            indirect: false,
            ..
        } = expr
            && !matches!(test_type, ParameterTestType::Unset)
            && context.runtime_variables.contains(&name)
        {
            // The default is lexical evidence from the empty branch, not a
            // known runtime value. One representative avoids multiplying loop
            // aggregates while retaining protected defaults and limited coverage.
            out.word.runtime_unknown = true;
            out.word.expands = true;
            return Ok(
                apply_default(expr, name, default_value.as_deref(), fragments, out)
                    .filter(|value| !value.is_empty()),
            );
        }
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
    let result = computed_value(expr, context, fragments, out, name, current)?;
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

mod transforms;
use transforms::{change_case, remove_pattern, replace_pattern, substring};

fn computed_value(
    expr: &ParameterExpr,
    context: &ExpansionContext<'_>,
    fragments: &[(&str, Fragment)],
    out: &mut Expanded,
    name: String,
    current: Option<String>,
) -> Result<Option<String>, CheckError> {
    use ParameterExpr::*;
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
                apply_default(expr, name, default_value.as_deref(), fragments, out)
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
            remove_pattern(expr, current, pattern(raw.as_deref()), context)?
        }
        Substring { offset, length, .. } => substring(current, offset, length, context, fragments)?,
        ReplaceSubstring {
            pattern: raw,
            replacement,
            match_kind,
            ..
        } => replace_pattern(
            current,
            pattern(Some(raw)),
            body(replacement.as_deref()),
            match_kind,
            context,
        )?,
        UppercaseFirstChar { pattern: raw, .. }
        | UppercasePattern { pattern: raw, .. }
        | LowercaseFirstChar { pattern: raw, .. }
        | LowercasePattern { pattern: raw, .. } => {
            change_case(expr, current, pattern(raw.as_deref()))
        }
        _ => None,
    };
    Ok(result)
}

fn indirect(expr: &ParameterExpr) -> Option<bool> {
    use ParameterExpr::*;
    match expr {
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
        | LowercasePattern { indirect, .. } => Some(*indirect),
        _ => None,
    }
}
