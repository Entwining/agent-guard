use super::{
    Inspection,
    messages::{SECRET_PRINT, TOKEN},
};
use crate::{
    Advice, CheckError, CheckErrorKind, CoverageGap, DenialRule, EffectRecord, EffectSource,
    Reason,
    filesystem::{self, Identity, Protection},
    record::{Effect, Via, Walk},
    shell,
    targets::{self, Target},
};

impl Inspection<'_> {
    fn hidden_search(&mut self) {
        self.effect(EffectRecord::HiddenContent);
        self.denial.get_or_insert_with(|| Reason {
            effect: "hidden recursive content search reaches protected environment files".into(),
            rule: DenialRule::HiddenSearch,
        });
    }
    fn effect(&mut self, effect: EffectRecord) {
        if !self.effects.contains(&effect) {
            self.effects.push(effect);
        }
    }
    fn gap(&mut self, value: CoverageGap) {
        if !self.gaps.contains(&value) {
            self.gaps.push(value);
        }
    }
    pub(super) fn target(
        &mut self,
        target: &Target,
        cwd: &str,
        source: EffectSource,
    ) -> Result<(), CheckError> {
        crate::check_deadline(self.deadline)?;
        let mut target = target.clone();
        if let Some(destination) = &target.relocation_destination
            && matches!(
                self.resolver
                    .relocation_destination(destination, cwd, self.probe)?,
                Identity::Protected(_)
            )
        {
            target.effect = Effect::Meta;
        }
        // Broad traversal is independent of a narrower credential identity.
        // Decide a lexical root before probes, then check resolved aliases below.
        let broad_access = target.via != Via::Items
            && !(target.via == Via::Tool && target.glob)
            && (target.walk != Walk::None || target.glob)
            && (target.effect != Effect::Name || target.glob);
        let lexical_broad = if broad_access {
            #[cfg(test)]
            {
                self.broad_root_queries += 1;
            }
            Some((
                target.path.clone(),
                self.resolver
                    .broad(&target.pattern_path(), &self.context.home, target.glob)?,
            ))
        } else {
            None
        };
        if lexical_broad.as_ref().is_some_and(|(_, matched)| *matched) {
            self.effect(EffectRecord::BroadRoot);
        }
        // A literal relative ~/ prefix stays anchored at cwd, including with a
        // runtime-derived suffix. Its link aliases still need normal resolution.
        let relative_tilde = target.unresolved.starts_with(&format!("{cwd}/~/"));
        let identity = if target.via == Via::Items {
            if !matches!(target.effect, Effect::Read | Effect::Change) {
                return Ok(());
            }
            let Some(kind) =
                self.resolver
                    .credential(&target.path, &self.context.home, target.glob)?
            else {
                return Ok(());
            };
            Identity::Protected(kind)
        } else if (target.expands || target.runtime_unknown)
            && !relative_tilde
            && filesystem::appdata_fragment(&target.path)
        {
            Identity::Protected(Protection::AppData)
        } else {
            self.resolver.target(&mut target, cwd, self.probe)?
        };
        match identity {
            Identity::Protected(kind) => {
                let touches = match kind {
                    Protection::AppData => target.effect != Effect::Name || target.glob,
                    Protection::SshPrivate => {
                        matches!(
                            target.effect,
                            Effect::Read | Effect::Write | Effect::Change | Effect::List
                        )
                    }
                    _ => matches!(target.effect, Effect::Read | Effect::Change),
                };
                if touches {
                    let rule = target_rule(&target, kind, &self.context.home, &mut self.resolver)?;
                    if kind == Protection::AppData {
                        self.appdata_reason.get_or_insert(rule);
                    }
                    self.effect(EffectRecord::ProtectedTarget {
                        protection: kind,
                        write: matches!(target.effect, Effect::Write | Effect::Change),
                        source,
                    });
                    self.denial.get_or_insert_with(|| Reason {
                        effect: if target.effect == Effect::Change {
                            format!("relocate protected {kind:?} content to an unprotected name")
                        } else if target.effect == Effect::Write {
                            format!("write protected location; {} is excluded", kind.effect())
                        } else {
                            kind.effect().to_owned()
                        },
                        rule,
                    });
                }
            }
            Identity::Bound => self.gap(CoverageGap::IdentityBound),
            Identity::InodeAlias => self.gap(CoverageGap::InodeAlias),
            Identity::InheritedInput => {
                self.gap(CoverageGap::UnresolvedTarget);
                if target.walk == Walk::Hidden && target.effect == Effect::Read {
                    self.hidden_search();
                }
            }
            Identity::Public(path) => {
                self.public_target(&target, &path, lexical_broad.as_ref(), broad_access)?
            }
        }
        Ok(())
    }
    fn public_target(
        &mut self,
        target: &Target,
        path: &str,
        lexical_broad: Option<&(String, bool)>,
        broad_access: bool,
    ) -> Result<(), CheckError> {
        let resolved_home = match self.resolver.home(self.probe)? {
            Identity::Public(path) => path,
            Identity::Bound => {
                self.gap(CoverageGap::IdentityBound);
                return Ok(());
            }
            Identity::InodeAlias => {
                self.gap(CoverageGap::InodeAlias);
                return Ok(());
            }
            Identity::InheritedInput => {
                self.gap(CoverageGap::UnresolvedTarget);
                return Ok(());
            }
            Identity::Protected(_) => self.context.home.clone(),
        };
        let resolved_broad = if let Some((lexical_path, matched)) = lexical_broad
            && lexical_path == path
            && resolved_home == self.context.home
        {
            *matched
        } else if broad_access {
            #[cfg(test)]
            {
                self.broad_root_queries += 1;
            }
            self.resolver
                .broad(&target.pattern_path(), &resolved_home, target.glob)?
        } else {
            false
        };
        if resolved_broad {
            self.effect(EffectRecord::BroadRoot);
            self.denial.get_or_insert_with(|| Reason {
                effect: format!(
                    "broad recursive root reaches protected locations; HOME {} is excluded",
                    self.context.home
                ),
                rule: DenialRule::Broad,
            });
        }
        if !resolved_broad
            && target.walk == Walk::Hidden
            && target.effect == Effect::Read
            && self
                .resolver
                .may_traverse(target, &resolved_home, self.probe)?
        {
            self.hidden_search();
        }
        Ok(())
    }
    pub(super) fn shell(
        &mut self,
        source: &str,
        cwd: &str,
        depth: usize,
    ) -> Result<(), CheckError> {
        crate::check_deadline(self.deadline)?;
        if depth > crate::limits::MAX_NESTING {
            return Err(CheckError {
                kind: CheckErrorKind::ResourceLimit,
            });
        }
        let mut target = Target::new(cwd.to_owned(), Effect::Read, Walk::None, Via::Cwd);
        match self.resolver.target(&mut target, cwd, self.probe)? {
            Identity::Protected(kind) => {
                if kind == Protection::AppData {
                    self.appdata_reason.get_or_insert(DenialRule::AppData);
                }
                self.effect(EffectRecord::ProtectedTarget {
                    protection: kind,
                    write: false,
                    source: EffectSource::Cwd,
                });
                self.denial.get_or_insert_with(|| Reason {
                    effect: format!("protected cwd: {}", kind.effect()),
                    rule: if kind == Protection::AppData {
                        DenialRule::AppData
                    } else {
                        DenialRule::Ssh
                    },
                });
            }
            Identity::Bound => self.gap(CoverageGap::IdentityBound),
            Identity::InodeAlias => self.gap(CoverageGap::InodeAlias),
            Identity::InheritedInput => self.gap(CoverageGap::UnresolvedTarget),
            Identity::Public(_) => {}
        }
        self.context
            .shell_observation_entries
            .set(self.context.shell_observation_entries.get() + 1);
        let observation = shell::observe_with_deadline(
            source,
            self.arm,
            &self.context.home,
            cwd,
            self.context.user.as_deref(),
            self.context.zsh_executor,
            self.deadline,
        )?;
        #[cfg(test)]
        {
            self.source_entries += observation.source_entries;
        }
        self.executable_qualifier |= observation.executable_qualifier;
        for value in observation.gaps {
            self.gap(value);
        }
        let mut hidden_listings = std::collections::BTreeSet::new();
        for (command_index, command) in observation.script.commands.into_iter().enumerate() {
            crate::check_deadline(self.deadline)?;
            self.inspect_command(command_index, &command, &mut hidden_listings, depth)?;
        }
        Ok(())
    }
    fn inspect_command(
        &mut self,
        command_index: usize,
        command: &shell::CommandRecord,
        hidden_listings: &mut std::collections::BTreeSet<(usize, usize)>,
        depth: usize,
    ) -> Result<(), CheckError> {
        let cwd = &command.cwd;
        let effects = targets::infer(
            command,
            cwd,
            crate::record::HostFacts {
                home: &self.context.home,
                user: self.context.user.as_deref(),
            },
        );
        self.pipeline_content(command.pipeline, &effects, hidden_listings);
        for value in effects.gaps {
            self.gap(
                if value == CoverageGap::ExecutorDivergence && !self.context.zsh_executor {
                    CoverageGap::UnsupportedDialectConstruct
                } else {
                    value
                },
            );
        }
        if effects.dump {
            self.effect(EffectRecord::EnvironmentDump);
            self.denial.get_or_insert_with(|| Reason {
                effect: "extract protected environment dump".into(),
                rule: DenialRule::Dump,
            });
        }
        if effects.variable {
            self.effect(EffectRecord::CredentialVariable);
            self.denial.get_or_insert_with(|| Reason {
                effect: "extract protected credential variable".into(),
                rule: DenialRule::Variable,
            });
        }
        if effects.token {
            self.effect(EffectRecord::HostingToken);
            self.denial.get_or_insert_with(|| Reason {
                effect: TOKEN.into(),
                rule: DenialRule::Token,
            });
        }
        if effects.keychain {
            self.effect(EffectRecord::Keychain);
            self.denial.get_or_insert_with(|| Reason { effect: "extract a password from the macOS Keychain; run the authorized client that consumes it without printing it".into(), rule: DenialRule::Keychain });
        }
        if effects.stored_secret {
            self.effect(EffectRecord::StoredSecret);
            self.denial.get_or_insert_with(|| Reason {
                effect: SECRET_PRINT.into(),
                rule: DenialRule::StoredSecret,
            });
        }
        if effects.trace {
            self.effect(EffectRecord::NetworkTrace);
            self.denial.get_or_insert_with(|| Reason { effect: "curl verbose or trace output can print authentication headers; drop -v and --trace and use a normal request".into(), rule: DenialRule::Trace });
        }
        if effects.hidden_content {
            self.hidden_search();
        }
        // Advice has shared public wording and cannot override protection.
        for (applies, advice) in [
            (effects.replace_advice, Advice::RgReplace),
            (effects.include_advice, Advice::RgInclude),
            (effects.bre_advice, Advice::RgBre),
        ] {
            if applies && !self.advice.contains(&advice) {
                self.advice.push(advice);
            }
        }
        for mut target in effects.targets {
            target.command = Some(command_index);
            self.target(
                &target,
                cwd,
                if target.via == Via::Cwd {
                    EffectSource::Cwd
                } else if depth > 0 || command.nested {
                    EffectSource::Nested
                } else {
                    EffectSource::Operand
                },
            )?;
        }
        for code in effects.inline {
            self.inline_code(&code, cwd)?;
        }
        for code in effects.code {
            self.shell(&code, cwd, depth + 1)?;
        }
        Ok(())
    }
    fn pipeline_content(
        &mut self,
        pipeline: Option<(usize, usize)>,
        effects: &targets::Effects,
        hidden_listings: &mut std::collections::BTreeSet<(usize, usize)>,
    ) {
        if let Some(pipeline) = pipeline {
            if effects.hidden_listing {
                hidden_listings.insert(pipeline);
            }
            if effects.consumes_listing && hidden_listings.contains(&pipeline) {
                self.effect(EffectRecord::HiddenContent);
                self.denial.get_or_insert_with(|| Reason {
                    effect:
                        "hidden listing with a content consumer reaches protected environment files"
                            .into(),
                    rule: DenialRule::HiddenSearch,
                });
            }
        }
    }
    fn inline_code(&mut self, code: &str, cwd: &str) -> Result<(), CheckError> {
        let previous_denial = self.denial.take();
        for path in targets::code_paths(code) {
            self.target(
                &Target::new(
                    filesystem::absolute_input(&path, cwd, &self.context.home),
                    Effect::Read,
                    Walk::None,
                    Via::Code,
                ),
                cwd,
                EffectSource::InlineCode,
            )?;
        }
        if self.denial.is_some() {
            self.gaps
                .retain(|g| *g != CoverageGap::InterpreterChosenRead);
            if let Some(reason) = self.denial.as_mut() {
                reason.effect = format!(
                    "CodeFile: inline interpreter token names a protected path; {} is excluded",
                    reason.effect
                );
                reason.rule = DenialRule::CodeFile;
            }
        } else {
            self.denial = previous_denial;
        }
        Ok(())
    }
}

fn target_rule(
    target: &Target,
    kind: Protection,
    home: &str,
    resolver: &mut filesystem::Resolver<'_>,
) -> Result<DenialRule, CheckError> {
    // Credential-content and SSH-scope refusals need distinct alternatives;
    // search scope must not be presented as an ordinary credential-file read.
    Ok(if kind == Protection::AppData {
        if !filesystem::appdata_reason(&target.path, home, target.glob)
            && resolver.broad(&target.path, home, target.glob)?
        {
            DenialRule::Broad
        } else {
            DenialRule::AppData
        }
    } else if target.effect == Effect::Change {
        DenialRule::ResourceChange
    } else if target.effect == Effect::Write {
        DenialRule::Ssh
    } else if target.walk == Walk::Hidden && target.effect == Effect::Read {
        DenialRule::HiddenSearch
    } else if target.effect == Effect::Read
        && resolver
            .credential(&target.path, home, target.glob)?
            .is_some()
    {
        if target.via == Via::Code {
            DenialRule::CodeFile
        } else if target.sends {
            DenialRule::Upload
        } else {
            DenialRule::File
        }
    } else if target.search {
        DenialRule::GrepSsh
    } else {
        DenialRule::Ssh
    })
}
