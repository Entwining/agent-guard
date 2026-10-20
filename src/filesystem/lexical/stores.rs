use super::super::{Protection, fold, glob, stores};
use super::{Lexical, Location};

impl Lexical {
    /// Whether walking `path`, or each directory its pattern names, reads a
    /// store, reading only files that `names` match when it bounds the walk
    /// and entering hidden names when `hidden`. HOME, its ancestors and
    /// `~/Library` belong to the broad-root rule.
    pub fn store_root(
        &mut self,
        path: &str,
        home: &str,
        patterned: bool,
        names: Option<&[String]>,
        hidden: bool,
    ) -> bool {
        self.prepare(home);
        let domain = &self.domains[home];
        let path = if patterned {
            glob::fold_pattern(path)
        } else {
            fold(path)
        };
        let path = path.trim_end_matches('/');
        let names: Option<Vec<_>> =
            names.map(|names| names.iter().map(|name| glob::fold_pattern(name)).collect());
        let matcher = &mut self.matcher;
        stores::reached(
            domain.stores.iter().chain(&self.locations),
            names.as_deref(),
            hidden,
            |dir| {
                let broad = dir == domain.library
                    || domain
                        .home
                        .strip_prefix(dir)
                        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'));
                !broad
                    && if patterned {
                        matcher.visible_path(path, dir, false)
                    } else {
                        dir == path
                    }
            },
        )
    }

    pub fn keeps_stores(&mut self, source: &str, destination: &str, home: &str) -> bool {
        self.prepare(home);
        stores::keeps_stores(
            &self.domains[home]
                .stores
                .iter()
                .chain(&self.locations)
                .collect::<Vec<_>>(),
            &fold(source),
            &fold(destination),
        )
    }

    /// Protects a folded path to, or inside, a credential file of a store that
    /// a variable or link moved, where no name identifies it.
    pub fn moved_store(&self, path: &str) -> Option<Protection> {
        stores::holds(self.locations.iter(), path).then_some(Protection::Credential)
    }

    /// Adds where HOME's store directories resolve. Earlier results were
    /// decided without them, so a new location clears every cached result.
    pub fn relocate(&mut self, moved: Vec<Location>) {
        let before = self.locations.len();
        for location in moved {
            if !self.locations.contains(&location) {
                self.locations.push(location);
            }
        }
        if self.locations.len() > before {
            for domain in self.domains.values_mut() {
                domain.paths.clear();
                domain.candidates.clear();
            }
        }
    }
}
