use super::super::{
    CheckError, FileKind, Identity, Probe, Protection, Resolution, absolute_input, checked_stat,
    normalize, rebase_pattern, resolve_pattern,
};
use super::Resolver;

impl Resolver<'_> {
    /// Whether moving or hard-linking `source` to `destination` leaves what it
    /// carries protected.
    pub(crate) fn relocation_keeps_protection(
        &mut self,
        source: &crate::record::Target,
        destination: &crate::record::Target,
        cwd: &str,
        probe: &mut dyn Probe,
    ) -> Result<bool, CheckError> {
        Ok(
            match self.relocation_destination(destination, cwd, probe)? {
                Identity::Protected(_) => true,
                Identity::Public(destination) => self.keeps_stores(source, &destination, probe)?,
                _ => false,
            },
        )
    }

    fn relocation_destination(
        &mut self,
        target: &crate::record::Target,
        cwd: &str,
        probe: &mut dyn Probe,
    ) -> Result<Identity, CheckError> {
        let raw = absolute_input(&target.unresolved, cwd, self.home);
        // A credential root protects reads below it, not names moved into it.
        if let Some(kind) = self
            .lexical
            .check(&target.pattern_path(), self.home, target.glob, true)?
            .filter(|kind| *kind != Protection::AppData)
        {
            return Ok(Identity::Protected(kind));
        }
        let resolved_home = match self.home(probe)? {
            Identity::Public(home) => home,
            other => return Ok(other),
        };
        // A relocation can replace the final directory entry. Resolve its
        // parent aliases, but do not borrow protection from the entry it replaces.
        let (parent, name) = raw.rsplit_once('/').unwrap_or(("/", raw.as_str()));
        let parent = if parent.is_empty() { "/" } else { parent };
        let parent_pattern = target
            .pattern
            .as_deref()
            .and_then(|pattern| pattern.rsplit_once('/').map(|(parent, _)| parent));
        let resolved = match resolve_pattern(
            parent,
            (self.home, Some(&resolved_home)),
            target.glob || target.expands,
            parent_pattern,
            self.table,
            probe,
            &mut self.lexical,
        )? {
            Resolution::Public(parent) => normalize(
                &format!("{}/{name}", parent.trim_end_matches('/')),
                cwd,
                self.home,
            ),
            Resolution::Protected(kind, _) => return Ok(Identity::Protected(kind)),
            Resolution::Bound => return Ok(Identity::Bound),
            Resolution::InodeAlias => return Ok(Identity::InodeAlias),
            Resolution::InheritedInput => return Ok(Identity::InheritedInput),
        };
        let pattern = target.pattern.as_deref().map_or_else(
            || resolved.clone(),
            |pattern| rebase_pattern(&raw, &resolved, pattern),
        );
        if resolved.ends_with("/.ssh") {
            return Ok(Identity::Protected(Protection::SshPrivate));
        }
        if let Some(kind) =
            self.lexical
                .check_both(&pattern, self.home, Some(&resolved_home), target.glob)?
        {
            Ok(Identity::Protected(kind))
        } else {
            Ok(Identity::Public(resolved))
        }
    }

    /// Whether moving the `source` store directory to the public, resolved
    /// `destination` keeps its credential files in a store. `mv` moves into a
    /// directory that already exists.
    pub(super) fn keeps_stores(
        &mut self,
        source: &crate::record::Target,
        destination: &str,
        probe: &mut dyn Probe,
    ) -> Result<bool, CheckError> {
        let name = source.path.rsplit('/').next().unwrap_or_default();
        let inside = format!("{}/{name}", destination.trim_end_matches('/'));
        let renamed = self
            .lexical
            .keeps_stores(&source.path, destination, self.home);
        let moved_inside = self.lexical.keeps_stores(&source.path, &inside, self.home);
        if !renamed && !moved_inside {
            return Ok(false);
        }
        let resolved_home = match self.home(probe)? {
            Identity::Public(home) => home,
            _ => return Ok(false),
        };
        Ok(
            match checked_stat(
                destination,
                self.home,
                &resolved_home,
                probe,
                &mut self.lexical,
            )?
            .map(|metadata| metadata.kind)
            {
                None => renamed,
                Some(FileKind::Directory) => moved_inside,
                Some(_) => false,
            },
        )
    }
}
