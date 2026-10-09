use super::{CheckError, Protection, glob};
use std::collections::BTreeMap;
use std::rc::Rc;

const APP_DATA_TREES: &[&str] = &[
    "containers",
    "group containers",
    "mobile documents",
    "cloudstorage",
];

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
    pub library_parts: Rc<[String]>,
    pub roots: Vec<Root>,
    pub broad_witnesses: Vec<String>,
    paths: BTreeMap<String, PathResults>,
    candidates: BTreeMap<String, [Option<Option<Protection>>; 4]>,
}

pub(super) struct Root {
    pub path: String,
    pub parts: Vec<String>,
}

impl Domain {
    pub fn new(home: &str) -> Self {
        let home = home.to_lowercase();
        let library = format!("{home}/library");
        let roots: Vec<_> = APP_DATA_TREES
            .iter()
            .map(|owner| {
                let path = format!("{library}/{owner}");
                Root {
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
            library_parts: library.split('/').map(str::to_owned).collect(),
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
    pub(super) deadline: Option<std::time::Instant>,
    #[cfg(test)]
    pub candidate_evaluations: usize,
    #[cfg(test)]
    pub path_evaluations: usize,
    #[cfg(test)]
    pub domain_preparations: usize,
    #[cfg(test)]
    pub classified_bytes: usize,
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
            #[cfg(test)]
            classified_bytes: 0,
        }
    }

    pub fn literal_walk(&mut self, home: &str, resolved_home: Option<&str>) -> LiteralWalk {
        self.prepare(home);
        let home_parts = self.domains[home].library_parts.clone();
        let resolved_parts = resolved_home
            .filter(|resolved| *resolved != home)
            .map(|resolved| {
                self.prepare(resolved);
                self.domains[resolved].library_parts.clone()
            });
        LiteralWalk::new(home_parts, resolved_parts)
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
                    self.classified_bytes += candidate.len();
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

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Parent {
    #[default]
    Other,
    Aws,
    Docker,
    Kube,
    Cargo,
    Config,
}

#[derive(Clone, Copy, Default)]
pub(super) struct LiteralState {
    parts: usize,
    library_mismatch: bool,
    pub appdata: bool,
    ssh_depth: Option<usize>,
    ssh_private: bool,
    pub environment: bool,
    environment_example: bool,
    credential_ancestor: bool,
    config_gh: bool,
    parent: Parent,
    pub credential: bool,
}

impl LiteralState {
    pub fn path(path: &str, library: &[String]) -> Self {
        path.split('/')
            .fold(Self::default(), |state, part| state.advance(part, library))
    }

    fn advance(mut self, original: &str, library: &[String]) -> Self {
        let part = original.to_lowercase();
        self.appdata |= !self.library_mismatch
            && self.parts == library.len()
            && APP_DATA_TREES.contains(&part.as_str());
        self.library_mismatch |= library
            .get(self.parts)
            .is_none_or(|expected| *expected != part);
        self.ssh_depth = match self.ssh_depth {
            Some(depth) => Some(depth + 1),
            None => (part == ".ssh").then_some(0),
        };
        self.ssh_private = self
            .ssh_depth
            .is_some_and(|depth| depth > 1 || depth == 1 && !super::ssh_public(original));
        self.environment |= part == ".env" || part.starts_with(".env.");
        self.environment_example = [".env.example", ".env.age"].contains(&part.as_str());
        self.credential_ancestor |= part == "private-keys-v1.d"
            || self.parts > 1 && self.parent == Parent::Cargo && part.starts_with("credentials");
        // Root reads remain role-dependent in the resolver; public children
        // must not inherit protection merely from a credential-directory name.
        self.credential = self.credential_ancestor
            || self.parts > 1 && self.parent == Parent::Aws && part.starts_with("credentials")
            || [".npmrc", ".netrc", ".git-credentials", ".pypirc", ".pgpass"]
                .contains(&part.as_str())
            || part.starts_with(".zprofile")
            || part.starts_with(".zsh_history")
            || part.starts_with("auth.json")
            || part.starts_with(".credentials.json")
            || part.ends_with(".pem")
            || part.ends_with(".key")
            || self.parts > 1
                && (self.parent == Parent::Docker && part == "config.json"
                    || self.parent == Parent::Kube && part == "config")
            || self.config_gh && part == "hosts.yml";
        self.config_gh = self.parts > 1 && self.parent == Parent::Config && part == "gh";
        self.parent = match part.as_str() {
            ".aws" => Parent::Aws,
            ".docker" => Parent::Docker,
            ".kube" => Parent::Kube,
            ".cargo" => Parent::Cargo,
            ".config" => Parent::Config,
            _ => Parent::Other,
        };
        self.parts += 1;
        self
    }

    pub fn protection(self) -> Option<Protection> {
        if self.appdata {
            Some(Protection::AppData)
        } else if self.ssh_private {
            Some(Protection::SshPrivate)
        } else if self.environment_example {
            None
        } else if self.environment {
            Some(Protection::Environment)
        } else if self.credential {
            Some(Protection::Credential)
        } else {
            None
        }
    }
}

pub(super) struct LiteralWalk {
    home: Rc<[String]>,
    resolved_home: Option<Rc<[String]>>,
    state: (LiteralState, LiteralState),
    ancestors: Vec<(LiteralState, LiteralState)>,
}

impl LiteralWalk {
    fn new(home: Rc<[String]>, resolved_home: Option<Rc<[String]>>) -> Self {
        let mut walk = Self {
            home,
            resolved_home,
            state: (LiteralState::default(), LiteralState::default()),
            ancestors: Vec::new(),
        };
        walk.restart();
        walk
    }

    pub fn restart(&mut self) {
        self.ancestors.clear();
        self.state = self.root();
    }

    fn root(&self) -> (LiteralState, LiteralState) {
        (
            LiteralState::default().advance("", &self.home),
            self.resolved_home
                .as_ref()
                .map_or_else(LiteralState::default, |home| {
                    LiteralState::default().advance("", home)
                }),
        )
    }

    pub fn push(&mut self, part: &str, projected_root: bool) {
        let (home, resolved) = self.state;
        self.ancestors.push(self.state);
        self.state = if projected_root {
            self.root()
        } else {
            (
                home.advance(part, &self.home),
                self.resolved_home
                    .as_ref()
                    .map_or(resolved, |home| resolved.advance(part, home)),
            )
        };
    }

    pub fn pop(&mut self) {
        if let Some(parent) = self.ancestors.pop() {
            self.state = parent;
        }
    }

    pub fn protection(&self) -> Option<Protection> {
        let (home, resolved) = self.state;
        home.protection().or_else(|| {
            self.resolved_home
                .as_ref()
                .and_then(|_| resolved.protection())
        })
    }
}

#[cfg(test)]
mod tests;
