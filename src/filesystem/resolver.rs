use super::{
    CheckError, FileKind, FirmlinkTable, Identity, Probe, Protection, Resolution, absolute_input,
    checked_stat, descriptor_path, lexical, near, normalize, rebase_pattern, resolve,
    resolve_pattern, same_file, sensitive_root, ssh_public,
};
use std::path::Path;

pub(crate) struct Resolver<'a> {
    home: &'a str,
    table: &'a FirmlinkTable,
    deadline: Option<std::time::Instant>,
    resolved_home: Option<Identity>,
    pub(super) lexical: lexical::Lexical,
    targets: std::collections::BTreeMap<TargetIdentity, (Identity, String, String, Option<String>)>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct TargetIdentity {
    pattern: Option<String>,
    unresolved: String,
    cwd: String,
    effect: crate::record::Effect,
    walk: crate::record::Walk,
    via: crate::record::Via,
    glob: bool,
    glob_hidden: bool,
    expands: bool,
    runtime_unknown: bool,
    search: bool,
}

impl<'a> Resolver<'a> {
    pub(crate) fn new(home: &'a str, table: &'a FirmlinkTable) -> Self {
        Self::with_deadline(home, table, None)
    }
    pub(crate) fn with_deadline(
        home: &'a str,
        table: &'a FirmlinkTable,
        deadline: Option<std::time::Instant>,
    ) -> Self {
        Self {
            home,
            table,
            deadline,
            resolved_home: None,
            lexical: lexical::Lexical::new(deadline),
            targets: std::collections::BTreeMap::new(),
        }
    }

    pub(crate) fn home(&mut self, probe: &mut dyn Probe) -> Result<Identity, CheckError> {
        crate::check_deadline(self.deadline)?;
        if let Some(home) = &self.resolved_home {
            return Ok(home.clone());
        }
        let home = match resolve(
            self.home,
            self.home,
            self.home,
            None,
            self.table,
            probe,
            &mut self.lexical,
        )? {
            Resolution::Public(path) => Identity::Public(normalize(&path, self.home, self.home)),
            Resolution::Protected(kind, _) => Identity::Protected(kind),
            Resolution::Bound => Identity::Bound,
            Resolution::InodeAlias => Identity::InodeAlias,
            Resolution::InheritedInput => Identity::InheritedInput,
        };
        self.resolved_home = Some(home.clone());
        Ok(home)
    }

    pub(crate) fn broad(
        &mut self,
        path: &str,
        home: &str,
        patterned: bool,
    ) -> Result<bool, CheckError> {
        self.lexical.broad(path, home, patterned)
    }

    pub(crate) fn may_traverse(
        &mut self,
        target: &crate::record::Target,
        resolved_home: &str,
        probe: &mut dyn Probe,
    ) -> Result<bool, CheckError> {
        if target.glob || target.expands || target.runtime_unknown {
            return Ok(true);
        }
        let kind = checked_stat(
            &target.path,
            self.home,
            resolved_home,
            probe,
            &mut self.lexical,
        )?
        .map(|metadata| metadata.kind);
        Ok(matches!(kind, None | Some(FileKind::Directory)))
    }

    pub(crate) fn credential(
        &mut self,
        path: &str,
        home: &str,
        patterned: bool,
    ) -> Result<Option<Protection>, CheckError> {
        Ok(self
            .lexical
            .check(path, home, patterned, true)?
            .filter(|kind| *kind != Protection::AppData)
            .or_else(|| {
                (!patterned && sensitive_root(path, home)).then_some(Protection::Credential)
            }))
    }

    pub(crate) fn target(
        &mut self,
        target: &mut crate::record::Target,
        cwd: &str,
        probe: &mut dyn Probe,
    ) -> Result<Identity, CheckError> {
        crate::check_deadline(self.deadline)?;
        let key = TargetIdentity {
            pattern: target.pattern.clone(),
            unresolved: target.unresolved.clone(),
            cwd: cwd.into(),
            effect: target.effect,
            walk: target.walk,
            via: target.via,
            glob: target.glob,
            glob_hidden: target.glob_hidden,
            expands: target.expands,
            runtime_unknown: target.runtime_unknown,
            search: target.search,
        };
        // One preflight shares one metadata observation. Roles and pattern
        // domains stay in the key, and a resolved alias must update both fields.
        if let Some((identity, path, unresolved, pattern)) = self.targets.get(&key) {
            target.path = path.clone();
            target.unresolved = unresolved.clone();
            target.pattern = pattern.clone();
            return Ok(identity.clone());
        }
        let identity = self.resolve_target(target, cwd, probe)?;
        self.targets.insert(
            key,
            (
                identity.clone(),
                target.path.clone(),
                target.unresolved.clone(),
                target.pattern.clone(),
            ),
        );
        Ok(identity)
    }

    pub(crate) fn relocation_destination(
        &mut self,
        target: &crate::record::Target,
        cwd: &str,
        probe: &mut dyn Probe,
    ) -> Result<Identity, CheckError> {
        let raw = absolute_input(&target.unresolved, cwd, self.home);
        if let Some(kind) = self.credential(&target.pattern_path(), self.home, target.glob)? {
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
            Resolution::Public(parent) => format!("{}/{name}", parent.trim_end_matches('/')),
            Resolution::Protected(kind, _) => return Ok(Identity::Protected(kind)),
            Resolution::Bound => return Ok(Identity::Bound),
            Resolution::InodeAlias => return Ok(Identity::InodeAlias),
            Resolution::InheritedInput => return Ok(Identity::InheritedInput),
        };
        let pattern = target.pattern.as_deref().map_or_else(
            || resolved.clone(),
            |pattern| rebase_pattern(&raw, &resolved, pattern),
        );
        if normalize(&resolved, cwd, self.home).ends_with("/.ssh") {
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

    fn resolve_target(
        &mut self,
        target: &mut crate::record::Target,
        cwd: &str,
        probe: &mut dyn Probe,
    ) -> Result<Identity, CheckError> {
        use crate::record::{Effect, Via};
        let raw = absolute_input(&target.unresolved, cwd, self.home);
        let path = normalize(&raw, cwd, self.home);
        if descriptor_path(&path) {
            return Ok(Identity::InheritedInput);
        }
        if path == "/.vol" || path.starts_with("/.vol/") {
            return Ok(Identity::InodeAlias);
        }
        if path.ends_with("/.ssh") {
            return Ok(Identity::Protected(Protection::SshPrivate));
        }
        if let Some(kind) = self.lexical.check(
            &target.pattern_path(),
            self.home,
            target.glob,
            target.glob_hidden,
        )? && !(target.glob
            && kind == Protection::Credential
            && !matches!(target.effect, Effect::Read | Effect::Change))
        {
            return Ok(Identity::Protected(kind));
        }
        if matches!(target.effect, Effect::Read | Effect::Change)
            && !target.glob
            && (target.via != Via::Cwd || target.search)
            && sensitive_root(&path, self.home)
        {
            return Ok(Identity::Protected(Protection::Credential));
        }
        if target.effect == Effect::Name && !target.glob || target.via == Via::Filter {
            return Ok(Identity::Public(path));
        }
        let resolved_home = match self.home(probe)? {
            Identity::Public(path) => path,
            other => return Ok(other),
        };
        let resolved = match resolve_pattern(
            &raw,
            (self.home, Some(&resolved_home)),
            target.glob || target.expands,
            target.pattern.as_deref(),
            self.table,
            probe,
            &mut self.lexical,
        )? {
            Resolution::Public(resolved) => {
                if resolved != raw {
                    if let Some(pattern) = &target.pattern {
                        target.pattern = Some(rebase_pattern(&raw, &resolved, pattern));
                    }
                    target.path = resolved.clone();
                    target.unresolved = resolved.clone();
                    resolved
                } else {
                    path.clone()
                }
            }
            Resolution::Protected(kind, resolved) => {
                target.path = resolved.clone();
                target.unresolved = resolved;
                return Ok(Identity::Protected(kind));
            }
            Resolution::Bound => return Ok(Identity::Bound),
            Resolution::InodeAlias => return Ok(Identity::InodeAlias),
            Resolution::InheritedInput => return Ok(Identity::InheritedInput),
        };
        self.resolved_identity(target, &path, resolved, cwd, &resolved_home, probe)
    }
    fn resolved_identity(
        &mut self,
        target: &crate::record::Target,
        path: &str,
        resolved: String,
        cwd: &str,
        resolved_home: &str,
        probe: &mut dyn Probe,
    ) -> Result<Identity, CheckError> {
        use crate::record::{Effect, Via, Walk};
        if let Some(kind) = self
            .lexical
            .check(
                &target.pattern_path(),
                self.home,
                target.glob,
                target.glob_hidden,
            )?
            .or(self.lexical.check(
                &target.pattern_path(),
                resolved_home,
                target.glob,
                target.glob_hidden,
            )?)
        {
            return Ok(Identity::Protected(kind));
        }
        if matches!(target.effect, Effect::Read | Effect::Change)
            && !target.glob
            && (target.via != Via::Cwd || target.search)
            && sensitive_root(&resolved, resolved_home)
        {
            return Ok(Identity::Protected(Protection::Credential));
        }
        // The broad-root owner wins before subordinate SSH metadata comparisons.
        let search = target.walk != Walk::None;
        if (search || target.glob)
            && self
                .lexical
                .broad(&target.pattern_path(), resolved_home, target.glob)?
        {
            return Ok(Identity::Public(resolved));
        }
        if matches!(
            target.effect,
            Effect::Read | Effect::Write | Effect::Change | Effect::List
        ) {
            match self.ssh_denied(path, &resolved, cwd, resolved_home, target.search, probe)? {
                Some(true) => return Ok(Identity::Protected(Protection::SshPrivate)),
                None => return Ok(Identity::Bound),
                Some(false) => {}
            }
        }
        Ok(Identity::Public(resolved))
    }
}

impl Resolver<'_> {
    pub(super) fn ssh_denied(
        &mut self,
        target: &str,
        resolved: &str,
        cwd: &str,
        resolved_home: &str,
        search: bool,
        probe: &mut dyn Probe,
    ) -> Result<Option<bool>, CheckError> {
        let home = self.home;
        let table = self.table;
        let ssh = format!("{home}/.ssh");
        let (root, protected_root) = match resolve(
            &ssh,
            cwd,
            home,
            Some(resolved_home),
            table,
            probe,
            &mut self.lexical,
        )? {
            Resolution::Public(root) => (root, false),
            Resolution::Protected(_, root) => (root, true),
            Resolution::Bound | Resolution::InodeAlias | Resolution::InheritedInput => {
                return Ok(None);
            }
        };
        let roots = if root == ssh {
            vec![ssh.as_str()]
        } else {
            vec![ssh.as_str(), root.as_str()]
        };
        let candidates = if target == resolved {
            vec![target]
        } else {
            vec![target, resolved]
        };
        if !candidates
            .iter()
            .any(|candidate| roots.iter().any(|root| near(candidate, root, search)))
        {
            return Ok(Some(false));
        }
        if protected_root {
            return Ok(Some(true));
        }
        for candidate in candidates {
            match self.ssh_candidate(candidate, target, &roots, resolved_home, search, probe)? {
                Some(false) => {}
                result => return Ok(result),
            }
        }
        Ok(Some(false))
    }
    fn ssh_candidate(
        &mut self,
        candidate: &str,
        target: &str,
        roots: &[&str],
        resolved_home: &str,
        search: bool,
        probe: &mut dyn Probe,
    ) -> Result<Option<bool>, CheckError> {
        let home = self.home;
        if self
            .lexical
            .check_both(candidate, home, Some(resolved_home), true)?
            .is_some()
        {
            return Ok(Some(true));
        }
        for root in roots {
            if same_file(
                candidate,
                root,
                home,
                resolved_home,
                probe,
                &mut self.lexical,
            )? {
                return Ok(Some(true));
            }
            if search {
                let mut parent = Path::new(root);
                while let Some(next) = parent.parent() {
                    parent = next;
                    let Some(spelling) = parent.to_str() else {
                        return Ok(None);
                    };
                    if same_file(
                        candidate,
                        spelling,
                        home,
                        resolved_home,
                        probe,
                        &mut self.lexical,
                    )? {
                        return Ok(Some(true));
                    }
                }
            }
            let mut parent = Path::new(candidate);
            while let Some(next) = parent.parent() {
                parent = next;
                let Some(spelling) = parent.to_str() else {
                    return Ok(None);
                };
                if !same_file(
                    spelling,
                    root,
                    home,
                    resolved_home,
                    probe,
                    &mut self.lexical,
                )? {
                    continue;
                }
                let base = Path::new(candidate)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                if Some(parent) != Path::new(candidate).parent() || !ssh_public(base) {
                    return Ok(Some(true));
                }
                let kind = checked_stat(target, home, resolved_home, probe, &mut self.lexical)?
                    .map(|metadata| metadata.kind);
                if kind == Some(FileKind::Directory) || search && kind != Some(FileKind::File) {
                    return Ok(Some(true));
                }
            }
        }
        Ok(Some(false))
    }
}
