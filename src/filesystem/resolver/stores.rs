use super::super::{CheckError, Probe, Resolution, resolve, sensitive_root, stores};
use super::Resolver;

impl Resolver<'_> {
    /// A store that HOME links elsewhere keeps its protection in the
    /// directory the link leads to, where nothing in its spelling names it.
    pub(super) fn locate_stores(
        &mut self,
        resolved_home: &str,
        probe: &mut dyn Probe,
    ) -> Result<(), CheckError> {
        let mut moved = Vec::new();
        for (dir, location) in stores::defaults(resolved_home) {
            // A store that resolves into a protected tree is protected there;
            // one past the link bound keeps only its spelling's protection.
            if let Resolution::Public(path) = resolve(
                &dir,
                resolved_home,
                self.home,
                Some(resolved_home),
                self.table,
                probe,
                &mut self.lexical,
            )? && path != dir
            {
                moved.push(location.moved(&path));
            }
        }
        self.lexical.relocate(moved);
        Ok(())
    }

    pub(super) fn credential_root(
        &mut self,
        path: &str,
        home: &str,
        names: Option<&[String]>,
        hidden: bool,
    ) -> Option<RootRead> {
        if sensitive_root(path) {
            Some(RootRead::Credentials)
        } else {
            self.lexical
                .store_root(path, home, false, names, hidden)
                .then_some(RootRead::Store)
        }
    }

    /// What reading `target` at `path` reads below a credential root. A walk
    /// from each directory a pattern names reads the stores below it.
    pub(super) fn root_read(
        &mut self,
        target: &crate::record::Target,
        path: &str,
        home: &str,
    ) -> Option<RootRead> {
        use crate::record::Walk;
        let names = target.bounds.as_deref();
        if target.glob {
            (target.walk != Walk::None
                && self.lexical.store_root(
                    &target.pattern_path(),
                    home,
                    true,
                    names,
                    target.walk_hidden,
                ))
            .then_some(RootRead::Store)
        } else {
            self.credential_root(path, home, names, target.walk_hidden)
        }
    }

    /// Whether reading `target` is refused for a client's store below it
    /// rather than for a credential directory or file it names.
    pub(crate) fn reads_store(&mut self, target: &crate::record::Target, home: &str) -> bool {
        self.root_read(target, &target.path, home) == Some(RootRead::Store)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RootRead {
    /// A directory whose name marks every file in it as a credential; name
    /// bounds do not narrow it.
    Credentials,
    /// A store's directory or one that holds it.
    Store,
}
