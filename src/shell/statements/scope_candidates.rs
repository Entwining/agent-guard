use super::super as shell;
use super::*;

impl Scope {
    pub(super) fn candidates(&self) -> BTreeMap<String, Vec<BindingValue>> {
        self.candidates_from(self.bindings.iter())
    }
    pub(super) fn named_candidates(&self, name: &str) -> Vec<BindingValue> {
        let prefix = format!("{name}[");
        self.candidates_from(
            self.bindings.get_key_value(name).into_iter().chain(
                self.bindings
                    .range(prefix.clone()..)
                    .take_while(|(key, _)| key.starts_with(&prefix)),
            ),
        )
        .remove(name)
        .unwrap_or_default()
    }
    pub(super) fn candidates_from<'a>(
        &self,
        bindings: impl Iterator<Item = (&'a String, &'a Binding)>,
    ) -> BTreeMap<String, Vec<BindingValue>> {
        let mut values: BTreeMap<String, Vec<BindingValue>> = BTreeMap::new();
        let mut seen = BTreeMap::<String, std::collections::HashSet<BindingValue>>::new();
        for (name, binding) in bindings {
            let base = name.split_once('[').map_or(name.as_str(), |(base, _)| base);
            let destination = values.entry(base.into()).or_default();
            for value in binding.values.iter() {
                let candidates = match value {
                    BindingValue::Array(array) => array.binding_values(),
                    value => vec![value.clone()],
                };
                for value in candidates {
                    #[cfg(test)]
                    self.candidate_work.set(self.candidate_work.get() + 1);
                    if seen.entry(base.into()).or_default().insert(value.clone()) {
                        destination.push(value);
                    }
                }
            }
        }
        values
    }
    pub fn values(&self) -> BTreeMap<String, Vec<String>> {
        self.candidates()
            .into_iter()
            .map(|(name, values)| {
                (
                    name,
                    values
                        .iter()
                        .filter_map(BindingValue::known)
                        .cloned()
                        .collect(),
                )
            })
            .collect()
    }
    pub fn contexts(&self) -> BTreeMap<String, String> {
        let mut contexts = self
            .bindings
            .iter()
            .filter_map(|(n, b)| {
                b.values
                    .first()
                    .and_then(BindingValue::lexical)
                    .map(|v| (n.clone(), v.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        contexts.extend(shell::arrays::scalar_contexts(self));
        contexts
    }

    pub fn pattern_contexts(
        &self,
        context: &BTreeMap<String, String>,
    ) -> BTreeMap<String, Vec<std::ops::Range<usize>>> {
        context
            .iter()
            .filter_map(|(name, text)| {
                let values = &self.bindings.get(name)?.values;
                let mut masks = values.iter().filter_map(|value| match value {
                    BindingValue::ShellMatches(value) | BindingValue::ShellDerived(value)
                        if &value.text == text =>
                    {
                        Some(&value.quoted_ranges)
                    }
                    _ => None,
                });
                let mut ranges = masks.next()?.clone();
                for mask in masks {
                    // Equal text from reachable branches may have different
                    // syntax: only bytes quoted in every branch stay literal.
                    ranges = ranges
                        .iter()
                        .flat_map(|range| {
                            mask.iter().filter_map(|other| {
                                let left = range.start.max(other.start);
                                let right = range.end.min(other.end);
                                (left < right).then_some(left..right)
                            })
                        })
                        .collect();
                }
                Some((name.clone(), ranges))
            })
            .collect()
    }

    pub(in crate::shell) fn unknown_parameters(
        &self,
        context: &BTreeMap<String, String>,
    ) -> BTreeSet<String> {
        self.bindings
            .iter()
            .filter(|(name, binding)| {
                binding.values.iter().any(|value| {
                    value.known().is_none()
                        && (value.lexical() == context.get(*name)
                            || matches!(value, BindingValue::Array(_)))
                })
            })
            .map(|(name, _)| name.clone())
            .collect()
    }
    pub(super) fn repeated_word(&self, word: &crate::record::Word) -> bool {
        word.vars.iter().any(|name| {
            self.bindings.get(name).is_some_and(|binding| {
                binding
                    .values
                    .iter()
                    .any(|value| matches!(value, BindingValue::RepeatedFields(_)))
            })
        })
    }
    pub(super) fn repetition_source(
        &self,
        word: &crate::record::Word,
    ) -> Option<Box<(String, String)>> {
        if !word.cardinality_unknown || word.expands || word.vars.len() != 1 {
            return None;
        }
        let name = &word.vars[0];
        let (prefix, suffix, _) = shell::words::parameter_affixes(&word.raw, name)?;
        if prefix.trim().is_empty()
            || !prefix.ends_with([' ', '\t', '\n'])
            || !suffix.is_empty() && !suffix.starts_with([' ', '\t', '\n'])
        {
            return None;
        }
        for value in self.bindings.get(name)?.values.iter() {
            let values = match value {
                BindingValue::RepeatedFields(repetition) => repetition.projections(),
                BindingValue::Known(value) => vec![value.clone()],
                _ => return None,
            };
            if values.iter().any(|value| {
                value
                    .chars()
                    .any(|ch| !ch.is_ascii_alphanumeric() && !"/._- \t".contains(ch))
            }) {
                return None;
            }
        }
        // Re-expand a whole operand at its command owner. Safe literal fields
        // cannot introduce shell syntax; that owner still rejects varying roles.
        Some(Box::new((
            name.clone(),
            format!("{prefix}${{{name}}}{suffix}"),
        )))
    }
    pub fn positional_sequences(&self) -> Option<Vec<Vec<crate::record::Word>>> {
        if self.defining
            && self
                .bindings
                .get("#")
                .is_some_and(|binding| binding.values.iter().any(|value| value.known().is_none()))
        {
            let mut word = crate::record::Word::literal("${@}".into());
            word.expands = true;
            word.runtime_unknown = true;
            word.vars.push("@".into());
            return Some(vec![vec![word]]);
        }
        let sequences = self
            .bindings
            .get("@")?
            .values
            .iter()
            .filter_map(|value| match value {
                BindingValue::Arguments(arguments) => Some(arguments.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        (!sequences.is_empty()).then_some(sequences)
    }
}
