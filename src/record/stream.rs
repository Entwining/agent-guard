use crate::{CheckError, CoverageGap, check_deadline};
use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{Hash, Hasher},
    rc::Rc,
    time::Instant,
};

pub(crate) type Guard = BTreeMap<usize, usize>;
pub(crate) type Origins = BTreeMap<String, Vec<Guard>>;
pub(crate) type Output = BTreeMap<usize, Flow>;

#[derive(Clone, Debug, Default)]
pub struct Flow {
    node: Option<Rc<Node>>,
    fingerprint: u64,
}

impl PartialEq for Flow {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        let mut seen = BTreeSet::new();
        while let Some((left, right)) = pending.pop() {
            if left.fingerprint != right.fingerprint {
                return false;
            }
            match (&left.node, &right.node) {
                (None, None) => {}
                (Some(left), Some(right)) => {
                    if Rc::ptr_eq(left, right) {
                        continue;
                    }
                    if !seen.insert((Rc::as_ptr(left), Rc::as_ptr(right))) {
                        continue;
                    }
                    match (left.as_ref(), right.as_ref()) {
                        (Node::Bytes(left, lg), Node::Bytes(right, rg))
                            if left == right && lg == rg => {}
                        (Node::Unknown, Node::Unknown) => {}
                        (Node::Sequence(left), Node::Sequence(right))
                        | (Node::Choice(left), Node::Choice(right))
                            if left.len() == right.len() =>
                        {
                            pending.extend(left.iter().zip(right))
                        }
                        (Node::Filter(left, lg, le), Node::Filter(right, rg, re))
                            if lg == rg && le == re =>
                        {
                            pending.push((left, right))
                        }
                        _ => return false,
                    }
                }
                _ => return false,
            }
        }
        true
    }
}
impl Eq for Flow {}
impl Hash for Flow {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.fingerprint.hash(state);
    }
}
#[cfg(test)]
impl Flow {
    pub(crate) fn ptr_eq(left: &Self, right: &Self) -> bool {
        match (&left.node, &right.node) {
            (Some(left), Some(right)) => Rc::ptr_eq(left, right),
            (None, None) => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
enum Node {
    Bytes(String, Guard),
    Unknown,
    Sequence(Vec<Flow>),
    Choice(Vec<Flow>),
    Filter(Flow, Guard, bool),
}

impl Node {
    fn fingerprint(&self) -> u64 {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        std::mem::discriminant(self).hash(&mut hash);
        match self {
            Self::Bytes(text, guard) => {
                text.hash(&mut hash);
                guard.hash(&mut hash);
            }
            Self::Unknown => {}
            Self::Sequence(parts) | Self::Choice(parts) => parts.hash(&mut hash),
            Self::Filter(flow, guard, exclude) => {
                flow.hash(&mut hash);
                guard.hash(&mut hash);
                exclude.hash(&mut hash);
            }
        }
        hash.finish()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    pub text: String,
    pub unknown: bool,
    pub known: bool,
    pub guard: Guard,
    pub cuts: Vec<usize>,
}

#[derive(Default)]
pub(crate) struct Builder {
    #[cfg(test)]
    pub nodes: usize,
    #[cfg(test)]
    pub pairs: usize,
    #[cfg(test)]
    pub visits: usize,
    #[cfg(test)]
    pub guard_pairs: usize,
    pub bounded: bool,
    pub deadline: Option<Instant>,
    next_choice: usize,
    next_channel: usize,
    candidates: BTreeMap<usize, (Rc<Node>, Rc<Vec<Candidate>>)>,
}

impl Builder {
    pub fn channel(&mut self) -> usize {
        let channel = self.next_channel;
        self.next_channel += 1;
        channel
    }

    pub fn outputs(&mut self, parts: &[Output], choice: bool) -> Output {
        let channels = parts
            .iter()
            .flat_map(|part| part.keys().copied())
            .collect::<std::collections::BTreeSet<_>>();
        channels
            .into_iter()
            .map(|channel| {
                let values = parts
                    .iter()
                    .map(|part| part.get(&channel).cloned().unwrap_or_default())
                    .collect();
                let flow = if choice {
                    self.choice(values)
                } else {
                    self.sequence(values)
                };
                (channel, flow)
            })
            .collect()
    }

    pub fn filter_output(&mut self, output: &Output, guard: &Guard, exclude: bool) -> Output {
        output
            .iter()
            .map(|(channel, flow)| {
                (
                    *channel,
                    self.node(Node::Filter(flow.clone(), guard.clone(), exclude)),
                )
            })
            .collect()
    }

    pub fn forget_output(&mut self, output: Output, choice: usize) -> Result<Output, CheckError> {
        output
            .into_iter()
            .map(|(channel, flow)| {
                let mut values = self.candidates(&flow)?;
                for value in &mut values {
                    value.guard.remove(&choice);
                }
                Ok((channel, self.materialize(values)))
            })
            .collect()
    }

    pub fn guards(&mut self, left: &[Guard], right: &[Guard]) -> Vec<Guard> {
        let mut values = Vec::new();
        for left in left {
            for right in right {
                #[cfg(test)]
                {
                    self.guard_pairs += 1;
                }
                if let Some(guard) = compatible(left, right)
                    && !values.contains(&guard)
                {
                    if values.len() == 512 {
                        self.bounded = true;
                    } else {
                        values.push(guard);
                    }
                }
            }
        }
        values
    }

    pub fn choice_id(&mut self) -> usize {
        let id = self.next_choice;
        self.next_choice += 1;
        id
    }

    fn node(&mut self, node: Node) -> Flow {
        #[cfg(test)]
        {
            self.nodes += 1;
        }
        Flow {
            fingerprint: node.fingerprint(),
            node: Some(Rc::new(node)),
        }
    }

    pub fn bytes(&mut self, text: String, guard: Guard) -> Flow {
        if text.is_empty() && guard.is_empty() {
            Flow::default()
        } else {
            self.node(Node::Bytes(text, guard))
        }
    }

    pub fn unknown(&mut self) -> Flow {
        self.node(Node::Unknown)
    }

    pub fn sequence(&mut self, mut parts: Vec<Flow>) -> Flow {
        parts.retain(|part| part.node.is_some());
        parts.dedup_by(|left, right| {
            matches!(left.node.as_deref(), Some(Node::Unknown))
                && matches!(right.node.as_deref(), Some(Node::Unknown))
        });
        match parts.len() {
            0 => Flow::default(),
            1 => parts.pop().unwrap_or_default(),
            _ => self.node(Node::Sequence(parts)),
        }
    }

    pub fn choice(&mut self, mut parts: Vec<Flow>) -> Flow {
        let mut distinct = Vec::new();
        for part in parts.drain(..) {
            if !distinct.contains(&part) {
                distinct.push(part);
            }
        }
        match distinct.len() {
            0 => self.node(Node::Choice(Vec::new())),
            1 => distinct.pop().unwrap_or_default(),
            _ => self.node(Node::Choice(distinct)),
        }
    }

    pub fn candidates(&mut self, flow: &Flow) -> Result<Vec<Candidate>, CheckError> {
        check_deadline(self.deadline)?;
        let Some(node) = &flow.node else {
            return Ok(vec![Candidate {
                text: String::new(),
                unknown: false,
                known: true,
                guard: Guard::new(),
                cuts: Vec::new(),
            }]);
        };
        #[cfg(test)]
        {
            self.visits += 1;
        }
        let key = Rc::as_ptr(node) as usize;
        if let Some((_, values)) = self.candidates.get(&key) {
            return Ok(values.as_ref().clone());
        }
        let values: Result<Vec<Candidate>, CheckError> = match node.as_ref() {
            Node::Bytes(text, guard) => Ok(vec![Candidate {
                text: text.clone(),
                unknown: false,
                known: !text.is_empty(),
                guard: guard.clone(),
                cuts: Vec::new(),
            }]),
            Node::Unknown => Ok(vec![Candidate {
                text: String::new(),
                unknown: true,
                known: false,
                guard: Guard::new(),
                cuts: vec![0],
            }]),
            Node::Choice(parts) => {
                let mut values = Vec::new();
                for part in parts {
                    for value in self.candidates(part)? {
                        self.insert(&mut values, value);
                    }
                }
                Ok(values)
            }
            Node::Filter(flow, guard, exclude) => Ok(self
                .candidates(flow)?
                .into_iter()
                .filter_map(|mut value| {
                    if *exclude {
                        (!guard
                            .iter()
                            .all(|(key, selected)| value.guard.get(key) == Some(selected)))
                        .then_some(value)
                    } else {
                        value.guard = compatible(&value.guard, guard)?;
                        Some(value)
                    }
                })
                .collect()),
            Node::Sequence(parts) => self.sequence_candidates(parts),
        };
        let values = values?;
        self.candidates
            .insert(key, (node.clone(), Rc::new(values.clone())));
        Ok(values)
    }

    fn sequence_candidates(&mut self, parts: &[Flow]) -> Result<Vec<Candidate>, CheckError> {
        let mut values = vec![Candidate {
            text: String::new(),
            unknown: false,
            known: false,
            guard: Guard::new(),
            cuts: Vec::new(),
        }];
        for part in parts {
            let right = self.candidates(part)?;
            let mut next = Vec::new();
            for left in &values {
                check_deadline(self.deadline)?;
                for right in &right {
                    #[cfg(test)]
                    {
                        self.pairs += 1;
                    }
                    if let Some(guard) = compatible(&left.guard, &right.guard) {
                        // A generated consumer input has the same byte boundary as
                        // an external one; check before allocating repeated output.
                        if left.text.len().saturating_add(right.text.len())
                            > crate::limits::MAX_INPUT_BYTES
                        {
                            self.bounded = true;
                            continue;
                        }
                        let mut cuts = left
                            .cuts
                            .iter()
                            .copied()
                            .chain(right.cuts.iter().map(|cut| cut + left.text.len()))
                            .collect::<Vec<_>>();
                        cuts.dedup();
                        self.insert(
                            &mut next,
                            Candidate {
                                text: format!("{}{}", left.text, right.text),
                                unknown: left.unknown || right.unknown,
                                known: left.known || right.known,
                                guard,
                                cuts,
                            },
                        );
                    }
                }
            }
            values = next;
        }
        Ok(values)
    }

    fn insert(&mut self, values: &mut Vec<Candidate>, value: Candidate) {
        if values.iter().any(|prior| {
            prior.text == value.text
                && prior.unknown == value.unknown
                && prior.known == value.known
                && prior.guard == value.guard
                && prior.cuts == value.cuts
        }) {
            return;
        }
        // This is the same finite candidate boundary as shell value expansion.
        if values.len() == 512 {
            self.bounded = true;
            return;
        }
        values.push(value);
    }

    pub fn materialize(&mut self, values: Vec<Candidate>) -> Flow {
        let mut parts = Vec::new();
        for value in values {
            let mut sequence = Vec::new();
            let mut previous = 0;
            sequence.push(self.bytes(String::new(), value.guard.clone()));
            let mut boundaries = value.cuts.clone();
            if boundaries.last() != Some(&value.text.len()) {
                boundaries.push(value.text.len());
            }
            for cut in boundaries {
                if cut > previous {
                    sequence
                        .push(self.bytes(value.text[previous..cut].into(), value.guard.clone()));
                }
                if cut != value.text.len() || value.cuts.contains(&cut) {
                    sequence.push(self.unknown());
                }
                previous = cut;
            }
            if value.unknown && value.cuts.is_empty() {
                sequence.push(self.unknown());
            }
            parts.push(self.sequence(sequence));
        }
        self.choice(parts)
    }

    pub fn gap(&self) -> Option<CoverageGap> {
        self.bounded.then_some(CoverageGap::InspectionBudget)
    }
}

pub(crate) fn compatible(left: &Guard, right: &Guard) -> Option<Guard> {
    if right
        .iter()
        .any(|(key, value)| left.get(key).is_some_and(|prior| prior != value))
    {
        return None;
    }
    let mut guard = left.clone();
    guard.extend(right.iter().map(|(key, value)| (*key, *value)));
    Some(guard)
}

#[cfg(test)]
mod tests;
