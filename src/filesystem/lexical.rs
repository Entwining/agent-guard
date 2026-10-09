use super::{CheckError, Protection, glob};
use std::collections::BTreeMap;

pub(super) struct Sensitive {
    pub pattern: &'static str,
    pub tail: Vec<&'static str>,
    pub witnesses: Vec<String>,
}

pub(super) struct Catalog {
    pub sensitive: Vec<Sensitive>,
    pub directories: Vec<&'static str>,
    pub directory_suffixes: Vec<String>,
}

pub(super) fn catalog() -> &'static Catalog {
    // Only the immutable protection catalog survives an evaluation.
    static CATALOG: std::sync::OnceLock<Catalog> = std::sync::OnceLock::new();
    CATALOG.get_or_init(|| {
        let sensitive = super::SENSITIVE
            .iter()
            .map(|pattern| {
                let tail: Vec<_> = pattern.trim_start_matches("**/").split('/').collect();
                let witnesses = tail.iter().map(|part| part.replace('*', "x")).collect();
                Sensitive {
                    pattern,
                    tail,
                    witnesses,
                }
            })
            .collect();
        let directories: Vec<_> = super::SENSITIVE
            .iter()
            .filter_map(|path| {
                path.trim_start_matches("**/")
                    .rsplit_once('/')
                    .map(|(dir, _)| dir)
                    .filter(|dir| !dir.contains('*'))
            })
            .collect();
        let directory_suffixes = directories.iter().map(|dir| format!("/{dir}")).collect();
        Catalog {
            sensitive,
            directories,
            directory_suffixes,
        }
    })
}

pub(super) struct Domain {
    pub home: String,
    pub library: String,
    pub roots: Vec<Root>,
    pub broad_witnesses: Vec<String>,
    paths: BTreeMap<String, PathResults>,
    candidates: BTreeMap<String, [Option<Option<Protection>>; 4]>,
}

pub(super) struct Root {
    pub path: String,
    pub prefix: String,
    pub parts: Vec<String>,
}

impl Domain {
    pub fn new(home: &str) -> Self {
        let home = home.to_lowercase();
        let library = format!("{home}/library");
        let roots: Vec<_> = [
            "containers",
            "group containers",
            "mobile documents",
            "cloudstorage",
        ]
        .iter()
        .map(|owner| {
            let path = format!("{library}/{owner}");
            Root {
                prefix: format!("{path}/"),
                parts: path.split('/').map(str::to_owned).collect(),
                path,
            }
        })
        .collect();
        let mut broad_witnesses = vec![home.clone(), library.clone()];
        for root in &roots {
            broad_witnesses.push(root.path.clone());
            broad_witnesses.push(format!("{}/x", root.path));
        }
        Self {
            home,
            library,
            roots,
            broad_witnesses,
            paths: BTreeMap::new(),
            candidates: BTreeMap::new(),
        }
    }
}

pub(super) struct Lexical {
    domains: BTreeMap<String, Domain>,
    matcher: glob::Matcher,
    deadline: Option<std::time::Instant>,
    #[cfg(test)]
    pub candidate_evaluations: usize,
    #[cfg(test)]
    pub path_evaluations: usize,
    #[cfg(test)]
    pub domain_preparations: usize,
}

#[derive(Default)]
struct PathResults {
    // Outer None means unobserved; Some(None) is a cached public spelling.
    protected: [Option<Option<Protection>>; 4],
    broad: [Option<bool>; 2],
}

impl Lexical {
    pub fn new(deadline: Option<std::time::Instant>) -> Self {
        Self {
            domains: BTreeMap::new(),
            matcher: glob::Matcher::default(),
            deadline,
            #[cfg(test)]
            candidate_evaluations: 0,
            #[cfg(test)]
            path_evaluations: 0,
            #[cfg(test)]
            domain_preparations: 0,
        }
    }

    pub fn check(
        &mut self,
        path: &str,
        home: &str,
        patterned: bool,
        hidden: bool,
    ) -> Result<Option<Protection>, CheckError> {
        crate::check_deadline(self.deadline)?;
        let mode = usize::from(patterned) * 2 + usize::from(hidden);
        if let Some(result) = self
            .domains
            .get(home)
            .and_then(|domain| domain.paths.get(path))
            .and_then(|result| result.protected[mode])
        {
            return Ok(result);
        }
        #[cfg(test)]
        {
            self.path_evaluations += 1;
        }
        self.prepare(home);
        let Some(domain) = self.domains.get_mut(home) else {
            unreachable!("prepare inserts the exact HOME domain");
        };
        let mut result = None;
        for candidate in glob::alternatives(path, patterned) {
            crate::check_deadline(self.deadline)?;
            let candidate_patterned = patterned || candidate != path;
            let mode = usize::from(candidate_patterned) * 2 + usize::from(hidden);
            let kind = if let Some(result) = domain
                .candidates
                .get(candidate.as_str())
                .and_then(|results| results[mode])
            {
                result
            } else {
                #[cfg(test)]
                {
                    self.candidate_evaluations += 1;
                }
                let result = super::lexical_candidate(
                    &candidate,
                    domain,
                    candidate_patterned,
                    hidden,
                    &mut self.matcher,
                );
                if let Some(results) = domain.candidates.get_mut(candidate.as_str()) {
                    results[mode] = Some(result);
                } else {
                    let mut results = [None; 4];
                    results[mode] = Some(result);
                    domain.candidates.insert(candidate, results);
                }
                result
            };
            if kind.is_some() {
                result = kind;
                break;
            }
        }
        if let Some(results) = domain.paths.get_mut(path) {
            results.protected[mode] = Some(result);
        } else {
            let mut results = PathResults::default();
            results.protected[mode] = Some(result);
            domain.paths.insert(path.to_owned(), results);
        }
        Ok(result)
    }

    pub fn check_both(
        &mut self,
        path: &str,
        home: &str,
        resolved_home: Option<&str>,
        patterned: bool,
    ) -> Result<Option<Protection>, CheckError> {
        let kind = self.check(path, home, patterned, true)?;
        if kind.is_some() {
            return Ok(kind);
        }
        match resolved_home {
            Some(resolved) if resolved != home => self.check(path, resolved, patterned, true),
            _ => Ok(None),
        }
    }

    pub fn broad(&mut self, path: &str, home: &str, patterned: bool) -> Result<bool, CheckError> {
        crate::check_deadline(self.deadline)?;
        let mode = usize::from(patterned);
        if let Some(result) = self
            .domains
            .get(home)
            .and_then(|domain| domain.paths.get(path))
            .and_then(|result| result.broad[mode])
        {
            return Ok(result);
        }
        self.prepare(home);
        let Some(domain) = self.domains.get_mut(home) else {
            unreachable!("prepare inserts the exact HOME domain");
        };
        let result =
            super::broad_prepared(path, domain, patterned, &mut self.matcher, self.deadline)?;
        if let Some(results) = domain.paths.get_mut(path) {
            results.broad[mode] = Some(result);
        } else {
            let mut results = PathResults::default();
            results.broad[mode] = Some(result);
            domain.paths.insert(path.to_owned(), results);
        }
        Ok(result)
    }

    fn prepare(&mut self, home: &str) {
        if !self.domains.contains_key(home) {
            #[cfg(test)]
            {
                self.domain_preparations += 1;
            }
            self.domains.insert(home.to_owned(), Domain::new(home));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_paths_and_prefixes_prepare_once_per_domain() {
        for width in [2, 4, 8, 16] {
            for depth in [1, 2, 4, 8] {
                let mut owner = Lexical::new(None);
                let mut paths = vec!["/".to_owned()];
                for _ in 0..depth {
                    paths.push(format!(
                        "{}/public",
                        paths.last().unwrap().trim_end_matches('/')
                    ));
                }
                for _ in 0..width {
                    for path in &paths {
                        for _ in 0..4 {
                            assert_eq!(
                                owner.check(path, "/synthetic/home", false, true).unwrap(),
                                None
                            );
                        }
                    }
                }
                assert_eq!(
                    owner.candidate_evaluations,
                    paths.len(),
                    "width={width}, depth={depth}"
                );
                assert_eq!(owner.domain_preparations, 1);
                assert_eq!(owner.path_evaluations, paths.len());
                println!(
                    "lexical width={width}, depth={depth}, candidates={}",
                    owner.candidate_evaluations
                );
            }
        }
    }

    #[test]
    fn overlapping_patterns_prepare_common_candidates_once() {
        for width in [2, 4, 8, 16] {
            for depth in [1, 2, 4, 8] {
                let mut owner = Lexical::new(None);
                for index in 0..width {
                    let prefix = format!("/{}", vec!["public"; depth].join("/"));
                    let path = format!("{prefix}/{{public{index},plain}}");
                    assert_eq!(owner.check(&path, "/h", true, false).unwrap(), None);
                }
                assert_eq!(owner.candidate_evaluations, width * 2 + 1);
                assert_eq!(owner.domain_preparations, 1);
                println!(
                    "overlap width={width}, depth={depth}, candidates={}",
                    owner.candidate_evaluations
                );
            }
        }
    }

    #[test]
    fn distinct_paths_reuse_components_and_bound_cache_storage() {
        for width in [2, 4, 8, 16] {
            for depth in [1, 2, 4, 8] {
                let mut owner = Lexical::new(None);
                for index in 0..width {
                    let path = format!(
                        "/h/{}/public{index}*/report*",
                        vec!["shared"; depth].join("/")
                    );
                    assert_eq!(owner.check(&path, "/h", true, true).unwrap(), None);
                    let misses = owner.matcher.component_evaluations;
                    assert_eq!(owner.check(&path, "/h", true, false).unwrap(), None);
                    assert_eq!(owner.matcher.component_evaluations, misses);
                    assert!(misses > 0);
                }
                let domain = &owner.domains["/h"];
                assert_eq!(domain.paths.len(), width);
                assert_eq!(domain.candidates.len(), width);
                assert_eq!(owner.candidate_evaluations, width * 2);
                let bytes: usize = domain
                    .paths
                    .keys()
                    .chain(domain.candidates.keys())
                    .map(String::len)
                    .sum();
                assert!(bytes <= width * (depth * 7 + 24) * 2);
                println!(
                    "unique width={width}, depth={depth}, candidates={}, key_bytes={bytes}, component_misses={}",
                    owner.candidate_evaluations, owner.matcher.component_evaluations
                );
            }
        }
    }

    #[test]
    fn cache_domains_preserve_spelling_visibility_and_deadlines() {
        let packet: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/rust-batch17e-patterns.json"
        ))
        .unwrap();
        let path = packet["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "ssh-remote-unquoted-glob")
            .unwrap()["input"]["command"]
            .as_str()
            .unwrap()
            .strip_prefix("ssh audit.invalid ")
            .unwrap()
            .replace("$H", "/h");
        let mut owner = Lexical::new(None);
        for home in ["/other", "/h"] {
            assert_eq!(
                owner.check(&path, home, true, false).unwrap().is_some(),
                home == "/h"
            );
            assert_eq!(
                owner.check(&path, home, false, false).unwrap().is_some(),
                home == "/h"
            );
            assert!(owner.broad(home, home, false).unwrap());
            assert!(!owner.broad("/project", home, false).unwrap());
        }
        assert_eq!(owner.domain_preparations, 2);
        // Results for hidden candidates and exact SSH spellings cannot leak
        // into the other visibility or case domain.
        assert_eq!(owner.check("/p/*rc", "/h", true, false).unwrap(), None);
        assert_eq!(
            owner.check("/p/*rc", "/h", true, true).unwrap(),
            Some(Protection::Credential)
        );
        assert_eq!(owner.check("/p/*rc", "/h", false, true).unwrap(), None);
        for (name, expected) in [("config", None), ("CONFIG", Some(Protection::SshPrivate))] {
            assert_eq!(
                owner
                    .check(&format!("/h/.ssh/{name}"), "/h", false, true)
                    .unwrap(),
                expected
            );
        }
        owner.deadline = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
        for (path, home, patterned, hidden) in [
            (path.as_str(), "/h", true, false),
            ("/h/.ssh/CONFIG", "/h", false, true),
        ] {
            assert_eq!(
                owner.check(path, home, patterned, hidden).unwrap_err().kind,
                crate::CheckErrorKind::Deadline
            );
        }
        assert_eq!(
            owner.broad("/h", "/h", false).unwrap_err().kind,
            crate::CheckErrorKind::Deadline
        );
    }

    #[test]
    fn link_walk_reuses_prefix_results_without_reusing_probes() {
        use super::super::{FirmlinkTable, Identity, Metadata, Probe, Resolver};
        use crate::record::{Effect, Target, Via, Walk};
        #[derive(Default)]
        struct Counted {
            readlinks: usize,
        }
        impl Probe for Counted {
            fn read_link(
                &mut self,
                _: &std::path::Path,
            ) -> std::io::Result<Option<std::path::PathBuf>> {
                self.readlinks += 1;
                Ok(None)
            }
            fn stat(&mut self, _: &std::path::Path) -> std::io::Result<Option<Metadata>> {
                panic!("unrelated public target must not stat");
            }
        }
        for width in [2, 4, 8, 16] {
            for depth in [1, 2, 4, 8] {
                let table = FirmlinkTable::from_text("");
                let mut resolver = Resolver::new("/h", &table);
                let mut probe = Counted::default();
                for index in 0..width {
                    for effect in [Effect::Read, Effect::Use] {
                        let path = format!("/p/{}/file{index}", vec!["public"; depth].join("/"));
                        let mut target = Target::new(path, effect, Walk::None, Via::Operand);
                        assert!(matches!(
                            resolver.target(&mut target, "/p", &mut probe).unwrap(),
                            Identity::Public(_)
                        ));
                    }
                }
                assert!(resolver.lexical.candidate_evaluations <= width + depth + 8);
                assert_eq!(resolver.lexical.domain_preparations, 1);
                assert!(probe.readlinks >= width * 2 * (depth + 2));
                println!(
                    "walk width={width}, depth={depth}, candidates={}, readlinks={}",
                    resolver.lexical.candidate_evaluations, probe.readlinks
                );
            }
        }
    }

    #[test]
    fn cached_public_prefix_does_not_hide_a_later_probe_fault() {
        use super::super::{FirmlinkTable, Identity, Metadata, Probe, Resolver};
        use crate::record::{Effect, Target, Via, Walk};
        struct Changing {
            fault: bool,
            calls: usize,
        }
        impl Probe for Changing {
            fn read_link(
                &mut self,
                path: &std::path::Path,
            ) -> std::io::Result<Option<std::path::PathBuf>> {
                self.calls += 1;
                if self.fault && path == std::path::Path::new("/p") {
                    return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
                }
                Ok(None)
            }
            fn stat(&mut self, _: &std::path::Path) -> std::io::Result<Option<Metadata>> {
                panic!("unexpected stat");
            }
        }
        let table = FirmlinkTable::from_text("");
        let mut owner = Resolver::new("/h", &table);
        let mut probe = Changing {
            fault: false,
            calls: 0,
        };
        let mut first = Target::new("/p/first".into(), Effect::Use, Walk::None, Via::Operand);
        assert!(matches!(
            owner.target(&mut first, "/p", &mut probe).unwrap(),
            Identity::Public(_)
        ));
        let before = probe.calls;
        probe.fault = true;
        let mut second = Target::new("/p/second".into(), Effect::Use, Walk::None, Via::Operand);
        assert_eq!(
            owner
                .target(&mut second, "/p", &mut probe)
                .unwrap_err()
                .kind,
            crate::CheckErrorKind::ProbeFault
        );
        assert_eq!(probe.calls, before + 1);
    }
}
