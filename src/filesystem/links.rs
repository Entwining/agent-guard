use super::{CheckError, CheckErrorKind, Probe, Resolution, lexical::Lexical, unfirmlink};
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
    table: &FirmlinkTable,
    probe: &mut dyn Probe,
    lexical: &mut Lexical,
) -> Result<Resolution, CheckError> {
    if super::descriptor_path(absolute) {
        return Ok(Resolution::InheritedInput);
    }
    let mut pending: VecDeque<String> = parts(absolute).collect();
    let mut prefix = "/".to_owned();
    let mut lower_prefix = "/".to_owned();
    // resolve_pattern supplies only the fixed prefix; link targets are literals too.
    let mut literal = lexical.literal_walk(home, resolved_home);
    let mut followed = false;
    let mut depth = 0;
    while let Some(part) = pending.pop_front() {
        crate::check_deadline(lexical.deadline)?;
        if part == ".." {
            let rebased = table.roots.contains(&lower_prefix);
            if rebased {
                prefix = unfirmlink(&prefix);
                lower_prefix = prefix.to_lowercase();
            }
            for path in [&mut prefix, &mut lower_prefix] {
                path.truncate(path.rfind('/').map_or(1, |at| at.max(1)));
            }
            if rebased {
                literal.restart();
                for component in parts(super::unfirmlink_path(&prefix)) {
                    crate::check_deadline(lexical.deadline)?;
                    #[cfg(test)]
                    {
                        lexical.classified_bytes += component.len();
                    }
                    literal.push(&component, false);
                }
            } else {
                literal.pop();
            }
        } else if part != "." {
            if prefix != "/" {
                prefix.push('/');
                lower_prefix.push('/');
            }
            prefix.push_str(&part);
            lower_prefix.push_str(&part.to_lowercase());
            #[cfg(test)]
            {
                lexical.classified_bytes += part.len();
            }
            literal.push(
                &part,
                super::unfirmlink_path(&prefix) == "/" && prefix != "/",
            );
        }
        let projected = super::unfirmlink_path(&prefix);
        if projected == "/.vol" || projected.starts_with("/.vol/") {
            return Ok(Resolution::InodeAlias);
        }
        if let Some(kind) = literal.protection() {
            return Ok(Resolution::Protected(kind, projected.to_owned()));
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
            if super::descriptor_path(&rewritten) {
                return Ok(Resolution::InheritedInput);
            }
            pending = parts(&rewritten).chain(pending).collect();
            prefix.clear();
            prefix.push('/');
            lower_prefix.clear();
            lower_prefix.push('/');
            literal.restart();
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
    super::strip_path_aliases(path)
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests;
