use super::*;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum HeaderValue {
    Unknown,
    Literal(String),
    Pattern(String, Vec<(usize, usize)>),
}

#[derive(Clone)]
struct HeaderKey {
    value: HeaderValue,
    #[cfg(test)]
    comparisons: std::rc::Rc<std::cell::Cell<usize>>,
}
impl HeaderKey {
    fn from_word(
        word: crate::record::Word,
        #[cfg(test)] comparisons: &Rc<std::cell::Cell<usize>>,
    ) -> Self {
        Self {
            value: if word.expands {
                HeaderValue::Unknown
            } else if word.globs {
                HeaderValue::Pattern(
                    word.text,
                    word.quoted_ranges
                        .into_iter()
                        .map(|r| (r.start, r.end))
                        .collect(),
                )
            } else {
                HeaderValue::Literal(word.text)
            },
            #[cfg(test)]
            comparisons: comparisons.clone(),
        }
    }

    fn binding(self) -> BindingValue {
        match self.value {
            HeaderValue::Unknown => BindingValue::RuntimeUnknown(None),
            HeaderValue::Literal(value) => BindingValue::Known(value),
            HeaderValue::Pattern(text, ranges) => BindingValue::ShellMatches(ShellValue {
                text,
                quoted_ranges: ranges.into_iter().map(|(start, end)| start..end).collect(),
            }),
        }
    }
}
impl PartialEq for HeaderKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for HeaderKey {}
impl PartialOrd for HeaderKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for HeaderKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        #[cfg(test)]
        {
            self.comparisons.set(self.comparisons.get() + 1);
        }
        self.value.cmp(&other.value)
    }
}

pub(super) struct LoopHeader {
    pub(super) values: Vec<BindingValue>,
    pub(super) literal: bool,
    pub(super) literal_values: Vec<String>,
    pub(super) ordered_values: Vec<(BindingValue, bool)>,
    pub(super) finite: bool,
    pub(super) count: Option<usize>,
    pub(super) has_words: bool,
}

impl<'a, 'b> Evaluator<'a, 'b> {
    pub(super) fn loop_header(
        &mut self,
        variable: Option<&str>,
        header: &[RawWord],
        scope: &mut Scope,
        context: (usize, bool),
    ) -> Result<LoopHeader, CheckError> {
        let (depth, nested) = context;
        let mut values = Vec::new();
        #[cfg_attr(
            test,
            expect(
                clippy::mutable_key_type,
                reason = "Only the test comparison counter mutates; HeaderValue alone determines key order."
            )
        )]
        let mut seen = BTreeSet::new();
        #[cfg(test)]
        let comparisons = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut literal = variable.is_some() && !header.is_empty();
        let mut literal_values = Vec::new();
        let mut ordered_values = Vec::new();
        let mut finite = true;
        let mut count = Some(0usize);
        for word in header {
            let mut width = 0;
            for expanded in self.expand(word, scope, depth)? {
                finite &= expanded.positional
                    || expanded.word.vars.iter().all(|name| {
                        scope.bindings.get(name).is_none_or(|binding| {
                            binding.values.iter().all(|value| value.known().is_some())
                        })
                    });
                literal &= (expanded.positional
                    || word.expansions.is_empty() && expanded.word.vars.is_empty())
                    && !expanded.word.expands
                    && !expanded.word.globs
                    && !expanded.word.cardinality_unknown
                    && !expanded.tilde
                    && expanded.nested.is_empty()
                    && expanded.arithmetic.is_empty();
                let members = if expanded.split.is_empty() {
                    std::slice::from_ref(&expanded.word)
                } else {
                    &expanded.split
                };
                for word in members {
                    ordered_values.push((
                        HeaderKey::from_word(
                            word.clone(),
                            #[cfg(test)]
                            &comparisons,
                        )
                        .binding(),
                        word.expands
                            || word.globs
                            || word.cardinality_unknown
                            || word.field_count_unknown,
                    ));
                }
                if expanded.positional {
                    literal_values.extend(expanded.split.iter().map(|word| word.text.clone()));
                } else {
                    literal_values.push(expanded.word.text.clone());
                }
                width = width.max(expanded.split.len().max(1));
                if expanded.word.expands || expanded.word.globs || expanded.word.cardinality_unknown
                {
                    count = None;
                }
                for word in expanded.split {
                    let value = HeaderKey::from_word(
                        word,
                        #[cfg(test)]
                        &comparisons,
                    );
                    if seen.insert(value.clone()) {
                        values.push(value.binding());
                    }
                }
                if !expanded.positional {
                    let value = HeaderKey::from_word(
                        expanded.word,
                        #[cfg(test)]
                        &comparisons,
                    );
                    if seen.insert(value.clone()) {
                        values.push(value.binding());
                    }
                }
            }
            count = count.map(|n| n + width);
            self.word_use(word, scope, depth, nested)?;
        }
        #[cfg(test)]
        {
            self.output.header_comparisons += comparisons.get();
        }
        Ok(LoopHeader {
            values,
            literal,
            literal_values,
            ordered_values,
            finite,
            count,
            has_words: !header.is_empty(),
        })
    }
}
