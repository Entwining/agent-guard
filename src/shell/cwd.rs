use super::{Operator, Statement, words};
use crate::CoverageGap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CwdPath {
    Logical(String),
    Physical { prefix: String, tail: String },
}

impl CwdPath {
    pub fn initial(path: &str) -> Self {
        let path = path
            .split('/')
            .filter(|s| *s != ".")
            .collect::<Vec<_>>()
            .join("/");
        Self::Logical(if path.is_empty() { "/".into() } else { path })
    }
    pub fn render(&self) -> String {
        match self {
            Self::Logical(path) => path.clone(),
            Self::Physical { prefix, tail } => {
                if tail.is_empty() {
                    format!("{prefix}/.")
                } else {
                    format!("{prefix}/./{tail}")
                }
            }
        }
    }
    pub fn change(&self, target: &str, physical: bool) -> Self {
        if target.starts_with('/') {
            return if physical {
                Self::Physical {
                    prefix: target.into(),
                    tail: String::new(),
                }
            } else {
                Self::Logical(clean(target))
            };
        }
        match self {
            Self::Logical(path) => {
                let raw = format!("{path}/{target}");
                if physical {
                    Self::Physical {
                        prefix: raw,
                        tail: String::new(),
                    }
                } else {
                    Self::Logical(clean(&raw))
                }
            }
            Self::Physical { prefix, tail } => {
                let steps = if tail.is_empty() {
                    target.into()
                } else {
                    format!("{tail}/{target}")
                };
                if physical {
                    return Self::Physical {
                        prefix: format!("{prefix}/{steps}"),
                        tail: String::new(),
                    };
                }
                let steps = clean(&steps);
                let mut parts = steps.split('/').peekable();
                let mut prefix = prefix.clone();
                while parts.peek() == Some(&"..") {
                    prefix.push_str("/..");
                    parts.next();
                }
                let tail = parts.filter(|s| *s != ".").collect::<Vec<_>>().join("/");
                Self::Physical { prefix, tail }
            }
        }
    }
}

fn clean(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|s| *s != "..") => {
                parts.pop();
            }
            ".." if absolute => {}
            part => parts.push(part),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".into()
    } else {
        joined
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Directory {
    pub current: CwdPath,
    pub alternatives: Vec<CwdPath>,
    pub failures: Option<Vec<CwdPath>>,
    pub gap: Option<CoverageGap>,
}

impl Directory {
    pub fn new(path: &str) -> Self {
        Self {
            current: CwdPath::initial(path),
            alternatives: Vec::new(),
            failures: None,
            gap: None,
        }
    }
    pub fn merge(&mut self, candidates: impl Iterator<Item = CwdPath>, home: &str) {
        let (alternatives, gap) = bounded(
            &self.current,
            self.alternatives
                .iter()
                .cloned()
                .chain(candidates)
                .collect(),
            home,
        );
        self.alternatives = alternatives;
        self.gap = self.gap.take().or(gap);
    }
    pub fn move_to(&mut self, targets: &[String], physical: bool, disputed: bool, home: &str) {
        let move_from = |cwd: &CwdPath| {
            let mut readings = Vec::new();
            for target in targets {
                readings.push(cwd.change(target, physical));
                if disputed {
                    readings.push(cwd.change(target, false));
                }
            }
            readings
        };
        let nexts = move_from(&self.current);
        let next = nexts[0].clone();
        let stayed = std::iter::once(self.current.clone())
            .chain(self.alternatives.iter().cloned())
            .collect::<Vec<_>>();
        let mut candidates = Vec::new();
        if let Some(failures) = &mut self.failures {
            extend_unique(failures, stayed);
        } else {
            candidates.extend(stayed);
        }
        candidates.extend(nexts.into_iter().skip(1));
        for cwd in &self.alternatives {
            candidates.extend(move_from(cwd));
        }
        let (alternatives, gap) = bounded(&next, candidates, home);
        self.alternatives = alternatives;
        self.gap = self.gap.take().or(gap);
        self.current = next;
    }
}

pub(super) fn extend_unique(
    paths: &mut Vec<CwdPath>,
    candidates: impl IntoIterator<Item = CwdPath>,
) {
    // Failure branches are identities, not executions. Repeated && joins must
    // not duplicate a failed directory into exponentially growing snapshots.
    for path in candidates {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
}

pub(super) fn bounded(
    current: &CwdPath,
    candidates: Vec<CwdPath>,
    home: &str,
) -> (Vec<CwdPath>, Option<CoverageGap>) {
    let mut result = Vec::new();
    for path in candidates {
        if &path != current && !result.contains(&path) {
            result.push(path);
        }
    }
    if result.len() > 16 {
        (
            vec![
                CwdPath::Logical(home.into()),
                CwdPath::Logical(format!("{home}/Library")),
            ],
            Some(CoverageGap::InspectionBudget),
        )
    } else {
        (result, None)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Movement {
    Unchanged,
    Moved,
    Uncertain,
}

fn movement(statement: &Statement) -> Movement {
    match statement {
        Statement::Command { argv, .. } => {
            let program = argv.first().and_then(|w| words::first_literal(&w.raw));
            match program.as_deref() {
                Some("popd") => Movement::Uncertain,
                Some("cd" | "pushd") => {
                    if argv
                        .get(1)
                        .and_then(|w| words::single_literal(&w.raw))
                        .is_some_and(|s| s != "-")
                    {
                        Movement::Moved
                    } else {
                        Movement::Uncertain
                    }
                }
                _ => Movement::Unchanged,
            }
        }
        Statement::Binary(Operator::And, left, right) => match (movement(left), movement(right)) {
            (Movement::Uncertain, _) | (_, Movement::Uncertain) => Movement::Uncertain,
            (Movement::Moved, _) | (_, Movement::Moved) => Movement::Moved,
            _ => Movement::Unchanged,
        },
        _ => Movement::Uncertain,
    }
}

pub(super) fn moved_on_success(statement: &Statement) -> bool {
    movement(statement) == Movement::Moved
}

pub(super) fn directory_success_guard(statement: &Statement) -> bool {
    match statement {
        Statement::Command { argv, .. } => argv
            .first()
            .and_then(|word| words::first_literal(&word.raw))
            .is_some_and(|program| matches!(program.as_str(), "cd" | "pushd" | "popd")),
        Statement::Binary(Operator::And, left, right) => {
            directory_success_guard(left) || directory_success_guard(right)
        }
        _ => false,
    }
}

#[cfg(test)]
std::thread_local! {
    static FAILURE_COPIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn count_failure_paths(paths: usize) {
    FAILURE_COPIES.with(|count| count.set(count.get() + paths));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_collection_has_polynomial_cost() {
        for zsh in [true, false] {
            let cost = |parts| {
                let source = std::iter::once("cd /synthetic/child")
                    .chain(std::iter::repeat_n("echo public", parts))
                    .collect::<Vec<_>>()
                    .join(" && ");
                FAILURE_COPIES.with(|count| count.set(0));
                let observation = crate::shell::observe(
                    &source,
                    crate::shell::Arm::Brush,
                    "/synthetic/home",
                    "/synthetic/work",
                    zsh,
                )
                .unwrap();
                assert!(observation.gaps.is_empty());
                FAILURE_COPIES.with(|count| count.get())
            };
            let small = cost(4);
            let large = cost(8);
            println!("zsh={zsh}: failure-path copies {small} -> {large}");
            assert!(small > 0);
            assert!(
                large <= small * 4,
                "assertion: failure-path copies {small} -> {large}"
            );
        }
    }
}
