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
    patterned: bool,
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
    let mut literal = (!patterned).then(|| lexical.literal_walk(home, resolved_home));
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
            if let Some(walk) = &mut literal {
                if rebased {
                    walk.restart();
                    for component in parts(super::unfirmlink_path(&prefix)) {
                        crate::check_deadline(lexical.deadline)?;
                        #[cfg(test)]
                        {
                            lexical.classified_bytes += component.len();
                        }
                        walk.push(&component, false);
                    }
                } else {
                    walk.pop();
                }
            }
        } else if part != "." {
            if prefix != "/" {
                prefix.push('/');
                lower_prefix.push('/');
            }
            prefix.push_str(&part);
            lower_prefix.push_str(&part.to_lowercase());
            if let Some(walk) = &mut literal {
                #[cfg(test)]
                {
                    lexical.classified_bytes += part.len();
                }
                walk.push(
                    &part,
                    super::unfirmlink_path(&prefix) == "/" && prefix != "/",
                );
            }
        }
        let projected = super::unfirmlink_path(&prefix);
        if projected == "/.vol" || projected.starts_with("/.vol/") {
            return Ok(Resolution::InodeAlias);
        }
        let kind = if let Some(walk) = &literal {
            walk.protection()
        } else {
            lexical.check_both(projected, home, resolved_home, patterned)?
        };
        if let Some(kind) = kind {
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
            if let Some(walk) = &mut literal {
                walk.restart();
            }
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
mod tests {
    use super::*;
    #[test]
    fn literal_walk_checks_deadline_after_each_probe() {
        struct Expiring {
            deadline: std::time::Instant,
            calls: usize,
        }
        impl Probe for Expiring {
            fn read_link(&mut self, _: &Path) -> std::io::Result<Option<std::path::PathBuf>> {
                self.calls += 1;
                if self.calls == 1 {
                    std::thread::sleep(
                        self.deadline
                            .saturating_duration_since(std::time::Instant::now()),
                    );
                    Ok(None)
                } else {
                    Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
                }
            }
            fn stat(&mut self, _: &Path) -> std::io::Result<Option<super::super::Metadata>> {
                panic!("link walk must not stat");
            }
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(25);
        let mut probe = Expiring { deadline, calls: 0 };
        let mut lexical = Lexical::new(Some(deadline));
        let result = follow(
            "/public/next",
            "/synthetic/home",
            None,
            false,
            &FirmlinkTable::from_text(""),
            &mut probe,
            &mut lexical,
        );
        assert_eq!(result.err().unwrap().kind, CheckErrorKind::Deadline);
        assert!(probe.calls <= 1);
        println!("probe_calls_before_deadline={}", probe.calls);
    }
    #[test]
    fn literal_walk_work_grows_with_path_bytes() {
        struct NoLinks;
        impl Probe for NoLinks {
            fn read_link(&mut self, _: &Path) -> std::io::Result<Option<std::path::PathBuf>> {
                Ok(None)
            }
            fn stat(&mut self, _: &Path) -> std::io::Result<Option<super::super::Metadata>> {
                panic!("public literal identity does not need stat");
            }
        }
        for depth in [128, 256, 512, 1024, 2048] {
            let path = format!("/public/{}file.txt", "p/".repeat(depth));
            let table = FirmlinkTable::from_text("");
            let mut owner = super::super::Resolver::new("/synthetic/home", &table);
            let mut target = crate::record::Target::new(
                path.clone(),
                crate::record::Effect::Read,
                crate::record::Walk::None,
                crate::record::Via::Operand,
            );
            assert_eq!(
                owner.target(&mut target, "/public", &mut NoLinks).unwrap(),
                super::super::Identity::Public(path.clone())
            );
            println!(
                "depth={depth}, path_bytes={}, lexical_bytes={}",
                path.len(),
                owner.lexical.classified_bytes
            );
            assert!(
                owner.lexical.classified_bytes <= 8 * path.len(),
                "prefix walk rescanned path bytes at depth {depth}"
            );
        }
    }
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
