use super::{CheckError, CheckErrorKind, Probe, Resolution, lexical_pattern, unfirmlink};
use std::{
    collections::{BTreeSet, VecDeque},
    path::Path,
};

/// Immutable host metadata. Evaluator input cannot override this catalog.
pub struct FirmlinkTable {
    roots: BTreeSet<String>,
}

impl FirmlinkTable {
    pub fn load() -> Result<Self, CheckError> {
        Self::from_read(std::fs::read_to_string("/usr/share/firmlinks"))
    }

    fn from_read(text: std::io::Result<String>) -> Result<Self, CheckError> {
        let text = text.map_err(|_| CheckError {
            kind: CheckErrorKind::ProbeFault,
        })?;
        Ok(Self::from_text(&text))
    }

    pub fn from_text(text: &str) -> Self {
        Self {
            roots: text
                .split('\n')
                .filter_map(|line| {
                    line.split('\t')
                        .nth(1)
                        .map(|root| format!("/system/volumes/data/{}", root.to_lowercase()))
                })
                .collect(),
        }
    }
}

pub(super) fn follow(
    absolute: &str,
    home: &str,
    resolved_home: Option<&str>,
    patterned: bool,
    table: &FirmlinkTable,
    probe: &mut dyn Probe,
) -> Result<Resolution, CheckError> {
    let mut pending: VecDeque<String> = parts(absolute).collect();
    let mut prefix = "/".to_owned();
    let mut followed = false;
    let mut depth = 0;
    while let Some(part) = pending.pop_front() {
        if part == ".." {
            if table.roots.contains(&prefix.to_lowercase()) {
                prefix = unfirmlink(&prefix);
            }
            prefix = Path::new(&prefix)
                .parent()
                .and_then(Path::to_str)
                .unwrap_or("/")
                .to_owned();
        } else if part != "." {
            if prefix != "/" {
                prefix.push('/');
            }
            prefix.push_str(&part);
        }
        let projected = unfirmlink(&prefix);
        let kind = lexical_pattern(&projected, home, patterned).or_else(|| {
            resolved_home.and_then(|home| lexical_pattern(&projected, home, patterned))
        });
        if let Some(kind) = kind {
            return Ok(Resolution::Protected(kind, projected));
        }
        let target = probe
            .read_link(Path::new(&prefix))
            .map_err(|_| CheckError {
                kind: CheckErrorKind::ProbeFault,
            })?;
        if let Some(target) = target {
            depth += 1;
            if depth > 8 {
                return Ok(Resolution::Bound);
            }
            let Some(target) = target.to_str() else {
                return Ok(Resolution::Bound);
            };
            let rewritten = if target.starts_with('/') {
                target.to_owned()
            } else {
                format!(
                    "{}/{target}",
                    Path::new(&prefix)
                        .parent()
                        .and_then(Path::to_str)
                        .unwrap_or("/")
                )
            };
            pending = parts(&rewritten).chain(pending).collect();
            prefix = "/".to_owned();
            followed = true;
        }
    }
    Ok(Resolution::Public(if followed {
        unfirmlink(&prefix)
    } else {
        absolute.to_owned()
    }))
}

fn parts(path: &str) -> impl Iterator<Item = String> + '_ {
    path.split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_read_fault_is_probe_fault() {
        let result = FirmlinkTable::from_read(Err(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied,
        )));
        assert!(matches!(
            result,
            Err(CheckError {
                kind: CheckErrorKind::ProbeFault
            })
        ));
    }
}
