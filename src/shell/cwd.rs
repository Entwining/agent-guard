use super::{Operator, Statement, words};
use crate::CoverageGap;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CwdPath {
    LoopUnknown(String),
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
            Self::LoopUnknown(base) => format!("{base}/${{__loop_cwd__}}"),
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
            Self::LoopUnknown(base) => Self::LoopUnknown(base.clone()),
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
    pub relative_growth: usize,
    pub current: CwdPath,
    pub alternatives: Rc<Vec<CwdPath>>,
    pub failures: Option<Rc<Vec<CwdPath>>>,
    pub gap: Option<CoverageGap>,
}

impl Directory {
    pub fn widen_unknown_loop(&mut self, initial: &Directory) {
        self.widen_loop(initial);
        let unknown = self
            .alternatives
            .iter()
            .find(|path| matches!(path, CwdPath::LoopUnknown(_)))
            .cloned();
        if let Some(unknown) = unknown {
            let prior = std::mem::replace(&mut self.current, unknown);
            Rc::make_mut(&mut self.alternatives).retain(|path| path != &self.current);
            extend_unique(Rc::make_mut(&mut self.alternatives), [prior]);
        }
    }
    pub fn widen_loop(&mut self, initial: &Directory) {
        let mut origins = self.alternatives.as_ref().clone();
        origins.push(self.current.clone());
        origins.push(initial.current.clone());
        origins.push(CwdPath::LoopUnknown(match &initial.current {
            CwdPath::LoopUnknown(base) => base.clone(),
            path => path.render(),
        }));
        self.alternatives = Rc::default();
        extend_unique(
            Rc::make_mut(&mut self.alternatives),
            origins.into_iter().filter(|path| path != &self.current),
        );
        self.gap = self.gap.take().or(Some(CoverageGap::UnresolvedTarget));
    }
    pub fn new(path: &str) -> Self {
        Self {
            relative_growth: 0,
            current: CwdPath::initial(path),
            alternatives: Rc::default(),
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
        if self.alternatives.as_ref() != &alternatives {
            self.alternatives = Rc::new(alternatives);
        }
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
            extend_unique(Rc::make_mut(failures), stayed);
        } else {
            candidates.extend(stayed);
        }
        candidates.extend(nexts.into_iter().skip(1));
        if !matches!(self.current, CwdPath::LoopUnknown(_))
            && !self
                .alternatives
                .iter()
                .any(|path| matches!(path, CwdPath::LoopUnknown(_)))
        {
            for cwd in self.alternatives.iter() {
                candidates.extend(move_from(cwd));
            }
        }
        let (alternatives, gap) = bounded(&next, candidates, home);
        if self.alternatives.as_ref() != &alternatives {
            self.alternatives = Rc::new(alternatives);
        }
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
    if result.len() > 16
        && !matches!(current, CwdPath::LoopUnknown(_))
        && !result
            .iter()
            .any(|path| matches!(path, CwdPath::LoopUnknown(_)))
    {
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

fn movement(statement: &Statement, #[cfg(test)] visits: &mut usize) -> Movement {
    #[cfg(test)]
    {
        *visits += 1;
    }
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
        Statement::Binary(Operator::And, left, right) => match (
            movement(
                left,
                #[cfg(test)]
                visits,
            ),
            movement(
                right,
                #[cfg(test)]
                visits,
            ),
        ) {
            (Movement::Uncertain, _) | (_, Movement::Uncertain) => Movement::Uncertain,
            (Movement::Moved, _) | (_, Movement::Moved) => Movement::Moved,
            _ => Movement::Unchanged,
        },
        _ => Movement::Uncertain,
    }
}

fn directory_success_guard(statement: &Statement, #[cfg(test)] visits: &mut usize) -> bool {
    #[cfg(test)]
    {
        *visits += 1;
    }
    match statement {
        Statement::Command { argv, .. } => argv
            .first()
            .and_then(|word| words::first_literal(&word.raw))
            .is_some_and(|program| matches!(program.as_str(), "cd" | "pushd" | "popd")),
        Statement::Binary(Operator::And, left, right) => {
            directory_success_guard(
                left,
                #[cfg(test)]
                visits,
            ) || directory_success_guard(
                right,
                #[cfg(test)]
                visits,
            )
        }
        _ => false,
    }
}

pub(super) struct LogicalContinuation<'a> {
    pub operator: &'a Operator,
    pub right: &'a Statement,
    pub qualified: bool,
    pub definition_on_success: bool,
}

pub(super) fn logical_continuations<'a>(
    statement: &'a Statement,
    #[cfg(test)] visits: &mut usize,
) -> (&'a Statement, Vec<LogicalContinuation<'a>>) {
    let mut spine = Vec::new();
    let mut current = statement;
    while let Statement::Binary(operator @ (Operator::And | Operator::Or), left, right) = current {
        spine.push((operator, right.as_ref()));
        current = left;
    }
    let mut moved = movement(
        current,
        #[cfg(test)]
        visits,
    );
    let mut guarded = directory_success_guard(
        current,
        #[cfg(test)]
        visits,
    );
    let mut continuations = Vec::with_capacity(spine.len());
    // Each left prefix inherits the prior summary; it never rewalks that tree.
    for (operator, right) in spine.into_iter().rev() {
        let and = matches!(operator, Operator::And);
        let qualified = and && moved == Movement::Moved;
        continuations.push(LogicalContinuation {
            operator,
            right,
            qualified,
            definition_on_success: and && (qualified || guarded),
        });
        if and {
            moved = match (
                moved,
                movement(
                    right,
                    #[cfg(test)]
                    visits,
                ),
            ) {
                (Movement::Uncertain, _) | (_, Movement::Uncertain) => Movement::Uncertain,
                (Movement::Moved, _) | (_, Movement::Moved) => Movement::Moved,
                _ => Movement::Unchanged,
            };
            guarded |= directory_success_guard(
                right,
                #[cfg(test)]
                visits,
            );
        } else {
            moved = Movement::Uncertain;
            guarded = false;
        }
    }
    continuations.reverse();
    (current, continuations)
}

#[cfg(test)]
mod tests;
