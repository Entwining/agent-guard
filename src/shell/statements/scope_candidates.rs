use super::super as shell;
use super::*;

impl Scope {
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
                    !matches!(value, BindingValue::Inherited | BindingValue::Unset)
                        && value.known().is_none()
                        && (value.lexical() == context.get(*name)
                            || matches!(value, BindingValue::Array(_)))
                })
            })
            .map(|(name, _)| name.clone())
            .collect()
    }
    /// The presence and text evidence that `context` cannot carry for
    /// `names`; `unset` holds the names the context projected as unset.
    pub(in crate::shell) fn facts<'a>(
        &self,
        names: impl IntoIterator<Item = &'a String>,
        context: &BTreeMap<String, String>,
        unknown: &BTreeSet<String>,
        unset: &BTreeSet<String>,
    ) -> shell::words::ContextFacts {
        let mut facts = shell::words::ContextFacts {
            unset: unset.clone(),
            inherited_unset: self.inherited_unset,
            cwd_unknown: self.directory.current.unknown(),
            ..Default::default()
        };
        for name in names {
            let Some(binding) = self.bindings.get(name) else {
                continue;
            };
            if binding
                .values
                .iter()
                .all(|value| value == &BindingValue::Unset)
                && !context.contains_key(name)
            {
                facts.unset.insert(name.clone());
            }
            let Some(text) = context.get(name).filter(|_| unknown.contains(name)) else {
                continue;
            };
            // Equal text from several values is known only where every
            // value that could have produced it knows it.
            let mut evidence: Option<Vec<crate::record::Gap>> = Some(Vec::new());
            let mut nonempty = false;
            for value in binding.values.iter() {
                if value.lexical() != Some(text) {
                    continue;
                }
                nonempty |= value
                    .gaps()
                    .is_some_and(|gaps| crate::record::nonempty_text(text.len(), gaps));
                if value.known().is_some() {
                    continue;
                }
                evidence = evidence
                    .zip(value.gaps())
                    .map(|(gaps, more)| crate::record::union_gaps(&gaps, more));
            }
            if let Some(gaps) = evidence.filter(|gaps| !gaps.is_empty()) {
                if nonempty && !crate::record::nonempty_text(text.len(), &gaps) {
                    facts.nonempty_values.insert(name.clone());
                }
                facts.evidence.insert(name.clone(), gaps);
            }
        }
        facts
    }
    /// The value a name takes in a branch that never assigned it.
    pub(in crate::shell) fn absent(&self) -> BindingValue {
        if self.inherited_unset {
            BindingValue::Unset
        } else {
            BindingValue::Inherited
        }
    }
    pub(in crate::shell) fn inherits_unset(&self) -> bool {
        self.inherited_unset
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
