use super::{Expanded, ExpansionContext, ParameterExpr, parameter_name};
use crate::CheckError;
use crate::record::Gap;
use brush_parser::word::{ParameterTestType, SubstringMatchKind};

pub(super) struct Fragment {
    /// The fragment's text when the shell fixes all of it.
    pub value: Option<String>,
    /// Text that is lexical evidence of the fragment when it is not exact.
    pub representative: Option<String>,
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
    definite: bool,
    used: &mut bool,
) -> Option<String> {
    let fragment = raw.and_then(|raw| fragments.iter().find(|(key, _)| *key == raw));
    let mut representative = false;
    let value = if raw.is_none() {
        Some(String::new())
    } else {
        fragment?.1.text(&mut representative)
    };
    *used |= representative;
    if let Some(value) = &value {
        if let Some((_, fragment)) = fragment {
            out.assignments.extend(fragment.assignments.clone());
        }
        if let ParameterExpr::AssignDefaultValues { test_type, .. } = expr {
            if definite && !representative {
                out.assignments.push((name, value.clone()));
            } else {
                // A colon test leaves either the old nonempty value or the
                // exact default.
                let nonempty = !matches!(test_type, ParameterTestType::Unset)
                    && !representative
                    && !value.is_empty();
                out.conditional_assignments.push((
                    name,
                    crate::shell::statements::ShellValue {
                        gaps: vec![Gap {
                            range: 0..value.len(),
                            nonempty,
                        }],
                        text: value.clone(),
                        quoted_ranges: Vec::new(),
                    },
                ));
            }
        }
    }
    value
}

impl Fragment {
    /// The fragment's exact text, or its representative, which sets
    /// `representative`.
    fn text(&self, representative: &mut bool) -> Option<String> {
        if self.value.is_none() {
            *representative |= self.representative.is_some();
        }
        self.value.clone().or_else(|| self.representative.clone())
    }
}

/// A parameter's expansion: its text and the runs of it that only represent
/// the runtime value.
pub(super) struct Value {
    pub text: String,
    pub gaps: Vec<Gap>,
    /// The text includes a representative of unknown output, whose
    /// whitespace is not a runtime field boundary.
    pub lexical: bool,
}

impl Value {
    fn represented(text: String, nonempty: bool) -> Self {
        Self {
            gaps: vec![Gap {
                range: 0..text.len(),
                nonempty,
            }],
            text,
            lexical: true,
        }
    }
    fn nonempty(&self) -> bool {
        crate::record::nonempty_text(self.text.len(), &self.gaps)
    }
}

pub(super) fn value(
    expr: &ParameterExpr,
    context: &ExpansionContext<'_>,
    fragments: &[(&str, Fragment)],
    out: &mut Expanded,
) -> Result<Option<Value>, CheckError> {
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
            .or_else(|| (name == "PWD" && !context.unset(name)).then(|| context.cwd.into()))
    };
    if !indirect && !out.assignments.iter().any(|(key, _)| *key == name) && context.unknown(&name) {
        let current = lookup(&name).filter(|_| context.unknown_variables.contains(&name));
        return unknown_value(expr, context, fragments, out, name, current);
    }
    if context.unknown_variables.contains(&name) {
        return Ok(None);
    }
    if indirect {
        let Some(value) = lookup(&name) else {
            return Ok(None);
        };
        name = value;
        if context.unknown(&name) {
            return Ok(None);
        }
    }
    let current = lookup(&name);
    if indirect && !out.word.vars.contains(&name) {
        out.word.vars.push(name.clone());
    }
    let hypothesis = context.facts.hypotheses.contains(&name);
    let mut representative = false;
    let result = computed_value(
        expr,
        context,
        fragments,
        out,
        name,
        current,
        &mut representative,
    )?;
    let inexact = hypothesis || representative;
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
    if inexact && result.is_some() {
        out.word.runtime_unknown = true;
        out.word.expands = true;
    }
    Ok(result.map(|text| Value {
        // Under its assumption a hypothesis computes the exact text.
        gaps: if inexact {
            vec![Gap {
                range: 0..text.len(),
                nonempty: !representative && !text.is_empty(),
            }]
        } else {
            Vec::new()
        },
        text,
        lexical: representative,
    }))
}

/// The expansion of a name whose value or presence the context does not
/// know, given the representative text of a value known to be set.
fn unknown_value(
    expr: &ParameterExpr,
    context: &ExpansionContext<'_>,
    fragments: &[(&str, Fragment)],
    out: &mut Expanded,
    name: String,
    current: Option<String>,
) -> Result<Option<Value>, CheckError> {
    use ParameterExpr::*;
    // A missing or unknown value leaves a default or alternative only as
    // lexical evidence of one branch. One representative avoids
    // multiplying loop aggregates while retaining protected defaults.
    let known_set = context.unknown_variables.contains(&name);
    let current = current.map(|text| Value {
        gaps: context.gaps(&name, &text, &mut out.uncertain_decisions),
        text,
        lexical: true,
    });
    let text = match expr {
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
            let colon = !matches!(test_type, ParameterTestType::Unset);
            if known_set && (!colon || current.as_ref().is_some_and(Value::nonempty)) {
                // The value's own text proves the test.
                current
            } else {
                apply_default(
                    expr,
                    name,
                    default_value.as_deref(),
                    fragments,
                    out,
                    false,
                    &mut true,
                )
                .filter(|value| !value.is_empty())
                .map(|text| Value::represented(text, colon))
            }
        }
        UseAlternativeValue {
            alternative_value, ..
        } => alternative_value
            .as_deref()
            .and_then(|raw| fragments.iter().find(|(key, _)| *key == raw))
            .and_then(|(_, fragment)| {
                out.assignments.extend(fragment.assignments.clone());
                fragment.text(&mut true)
            })
            .filter(|value| !value.is_empty())
            .map(|text| Value::represented(text, false)),
        ParameterLength { .. } => None,
        // A pattern or offset applied to a representative gives the
        // representative of the result. Where the value is sure to be
        // nonempty, the runtime pattern may match it differently and leave
        // it whole, so the alternate reading keeps the value.
        _ => match current {
            Some(current) if current.nonempty() && context.facts.alternate => Some(current),
            Some(current) => {
                out.uncertain_decisions |= current.nonempty();
                computed_value(
                    expr,
                    context,
                    fragments,
                    out,
                    name,
                    Some(current.text),
                    &mut true,
                )?
                .map(|text| Value::represented(text, false))
            }
            _ => None,
        },
    };
    if text.is_some() {
        out.word.runtime_unknown = true;
        out.word.expands = true;
    }
    Ok(text)
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
    representative: &mut bool,
) -> Result<Option<String>, CheckError> {
    use ParameterExpr::*;
    let get = |raw: &str| {
        fragments
            .iter()
            .find(|(key, _)| *key == raw)
            .map(|(_, value)| value)
    };
    let (mut used, mut pattern_used) = (false, false);
    let mut body =
        |raw: Option<&str>| raw.map_or(Some(String::new()), |raw| get(raw)?.text(&mut used));
    let mut pattern = |raw: Option<&str>| {
        raw.map_or(Some(String::new()), |raw| {
            let fragment = get(raw)?;
            fragment.text(&mut pattern_used)?;
            Some(fragment.pattern.clone())
        })
    };
    let present = |test: &ParameterTestType| {
        current
            .as_ref()
            .is_some_and(|value| matches!(test, ParameterTestType::Unset) || !value.is_empty())
    };
    let unchanged = current.clone();
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
                apply_default(
                    expr,
                    name,
                    default_value.as_deref(),
                    fragments,
                    out,
                    true,
                    representative,
                )
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
    *representative |= used || pattern_used;
    // The runtime pattern may match differently or not at all; the alternate
    // reading keeps the value as lexical evidence.
    out.uncertain_decisions |= pattern_used;
    if pattern_used && context.facts.alternate {
        return Ok(unchanged);
    }
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
