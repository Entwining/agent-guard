use crate::{
    Advice, CheckError, CheckErrorKind, Coverage, CoverageGap, DenialRule, Disposition,
    EffectRecord, EffectSource, Evaluation, Outcome, Reason, Recovery, RecoveryStep,
    adapters::{self, Consumer, Operation},
    filesystem::{self, Identity, Probe, Protection},
    limits::MAX_INPUT_BYTES,
    record::{Effect, Via, Walk},
    shell::{self, Arm},
    targets::{self, Target},
};

// Public sentences carry the safe alternatives; diagnostic effect text
// stays internal and must not replace them in consumer output.
const APPDATA: &str = "This reads a protected macOS app-data directory. Name a specific non-sensitive file under ~/Library/Application Support instead, or ask the user to inspect the protected file and share the needed fact.";
const BROAD: &str = "A scan rooted at the home directory or ~/Library reaches every app-data entry. Scope the scan to a project path.";
const FILE: &str = "This reads a credential or environment file. If a client the guard models only needs to use the file, pass it through that program's own option, such as `--env-file`, `--kubeconfig`, or `ssh -i`; the guard does not control what the client does with the contents. Otherwise read a non-sensitive config file, or ask the user to inspect the file and share only the fact needed.";
const CODE_FILE: &str = "This inline code names a credential or environment file. To write text that mentions the file, use the Write or Edit tool; to run code that needs its values, pass the file through a modelled runtime's option, such as `node --env-file=.env`, though the guard does not control what the runtime does with the contents; otherwise ask the user to inspect the file and share only the fact needed.";
const HIDDEN_SEARCH: &str = "A recursive search that includes hidden files can read credentials. Use default rg on a project path, or search an exact non-sensitive file without recursive or hidden-file flags.";
const DUMP: &str = "This dumps environment or shell variables, including secrets. Name the non-sensitive variable needed and read only that variable.";
const VARIABLE: &str = "This prints the value of a credential variable. Ask the user for the specific non-sensitive fact needed, or let the authorized client consume the credential without printing it.";
const TOKEN: &str = "This prints a Git hosting token. Use auth status without token-display flags; if authentication needs repair, ask the user to update the credential in their terminal.";
const KEYCHAIN: &str = "This extracts a password from the macOS Keychain. State the intended use and run the authorized client that consumes it without printing it.";
const SECRET_PRINT: &str = "This prints a stored secret or access token. Run the command that uses the credential without printing it, or ask the user to run it in their own terminal and share only the non-secret fact needed.";
const TRACE: &str = "curl verbose or trace output can print HTTP headers including Authorization. Drop -v and --trace; use a normal curl request for the needed result.";
const UPLOAD: &str = "This sends the contents of a credential file. Send only the required non-sensitive fields explicitly, and let the client obtain authentication from its normal credential source.";
const SSH: &str = "This reads private material in the named .ssh directory or its filesystem alias. Search public material in the project or request the exact public key or client-config path; ask the user to inspect private material locally if a specific non-sensitive fact is needed.";
const GREP_SSH: &str = "Grep would search private material in the named .ssh directory or its filesystem alias. Narrow the search to a project directory or an exact public key, client config, allowed_signers, or known_hosts file.";

impl DenialRule {
    pub fn message(self) -> &'static str {
        match self {
            Self::ResourceChange => {
                "This changes a protected credential, environment or SSH resource. Use a non-sensitive source and destination, or ask the user to perform the change in their own terminal and share only the non-sensitive result."
            }
            Self::AppData => APPDATA,
            Self::Broad => BROAD,
            Self::File => FILE,
            Self::CodeFile => CODE_FILE,
            Self::HiddenSearch => HIDDEN_SEARCH,
            Self::Dump => DUMP,
            Self::Variable => VARIABLE,
            Self::Token => TOKEN,
            Self::Keychain => KEYCHAIN,
            Self::StoredSecret => SECRET_PRINT,
            Self::Trace => TRACE,
            Self::Upload => UPLOAD,
            Self::Ssh => SSH,
            Self::GrepSsh => GREP_SSH,
        }
    }
}

pub(crate) fn refusal_message(cause: &CoverageGap) -> &'static str {
    match cause {
        CoverageGap::InodeAlias => {
            "The guard cannot inspect an inode-addressed /.vol path. Name the file by its ordinary file path, then recheck the call."
        }
        CoverageGap::InspectionBudget => {
            "This command exceeds the guard's inspection budget. Split loops or function calls into smaller commands with explicit public paths, then recheck each command."
        }
        CoverageGap::IdentityBound => {
            "The guard cannot resolve this path through bounded or cyclic aliases. Name the file by an ordinary absolute path without the cyclic or deep alias chain, then recheck the call."
        }
        CoverageGap::InputByteLimit => {
            "This event exceeds the input byte limit. Split the call into smaller requests, then recheck each request."
        }
        CoverageGap::NestingLimit => {
            "Shell nesting exceeds the supported depth of 64. Shorten the nesting or split the command, then recheck each command."
        }
        CoverageGap::AbsoluteCwdRequired => {
            "The event requires an absolute cwd. Use an absolute cwd, then recheck the call."
        }
        CoverageGap::InvalidEncoding => {
            "The event is not valid UTF-8. Encode the request as UTF-8, then recheck the call."
        }
        CoverageGap::UnsupportedShellSyntax => {
            "The agent guard cannot inspect this shell syntax. Rewrite it as a Bash-compatible command with explicit paths, or run a narrower command that the guard can inspect."
        }
        CoverageGap::ExecutionOwnerUnavailable => {
            "The agent guard cannot verify the execution owner for this operation. Have the owner establish and verify the execution boundary, then recheck the call."
        }
        _ => {
            "The agent guard cannot inspect this unsupported or unresolved operation. Replace the unsupported construct with a Bash-compatible command naming an explicit public path, then recheck it."
        }
    }
}

/// Consumer and host facts. Task intent and agent continuations are not guard inputs.
pub struct Context {
    pub consumer: Consumer,
    pub home: String,
    pub user: Option<String>,
    pub cwd: String,
    pub zsh_executor: bool,
    pub require_execution_owner: bool,
    pub shell_observation_entries: std::cell::Cell<usize>,
}

pub struct Event<'a> {
    pub bytes: &'a [u8],
    pub context: &'a Context,
    pub probe: &'a mut dyn Probe,
}

pub fn evaluate(event: Event<'_>) -> Result<Evaluation, CheckError> {
    evaluate_with_arm(event, Arm::Brush)
}

pub fn evaluate_with_arm(event: Event<'_>, arm: Arm) -> Result<Evaluation, CheckError> {
    evaluate_with_catalog_loader(
        event,
        arm,
        filesystem::FirmlinkTable::load,
        None,
        adapters::Protocol::Tool,
    )
}

pub fn evaluate_with_deadline(
    event: Event<'_>,
    deadline: std::time::Instant,
) -> Result<Evaluation, CheckError> {
    evaluate_with_catalog_loader(
        event,
        Arm::Brush,
        filesystem::FirmlinkTable::load,
        Some(deadline),
        adapters::Protocol::Tool,
    )
}

pub(crate) fn evaluate_native(
    event: Event<'_>,
    deadline: std::time::Instant,
) -> Result<Evaluation, CheckError> {
    if event.bytes.len() > MAX_INPUT_BYTES {
        return Ok(native_refusal(CoverageGap::InputByteLimit));
    }
    match evaluate_with_catalog_loader(
        event,
        Arm::Brush,
        filesystem::FirmlinkTable::load,
        Some(deadline),
        adapters::Protocol::Native,
    ) {
        Err(CheckError {
            kind: CheckErrorKind::ResourceLimit,
        }) => Ok(native_refusal(CoverageGap::NestingLimit)),
        Err(CheckError {
            kind: CheckErrorKind::RelativeCwd,
        }) => Ok(native_refusal(CoverageGap::AbsoluteCwdRequired)),
        Err(CheckError {
            kind: CheckErrorKind::InvalidEncoding,
        }) => Ok(native_refusal(CoverageGap::InvalidEncoding)),
        result => result,
    }
}

fn native_refusal(cause: CoverageGap) -> Evaluation {
    Evaluation {
        outcome: Outcome::CoverageInsufficient {
            cause: cause.clone(),
            disposition: Disposition::RejectUnsupportedSyntax,
            recovery: None,
        },
        coverage: Coverage::LimitedPreflight(vec![cause]),
        effects: Vec::new(),
    }
}

/// Typed host metadata for isolated evaluations; hook request input has no catalog field.
pub fn evaluate_with_catalog(
    event: Event<'_>,
    arm: Arm,
    catalog: filesystem::FirmlinkTable,
) -> Result<Evaluation, CheckError> {
    evaluate_with_catalog_loader(event, arm, || Ok(catalog), None, adapters::Protocol::Tool)
}

fn evaluate_with_catalog_loader(
    event: Event<'_>,
    arm: Arm,
    load: impl FnOnce() -> Result<filesystem::FirmlinkTable, CheckError>,
    deadline: Option<std::time::Instant>,
    protocol: adapters::Protocol,
) -> Result<Evaluation, CheckError> {
    crate::check_deadline(deadline)?;
    let Event {
        bytes,
        context,
        probe,
    } = event;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(CheckError {
            kind: CheckErrorKind::ResourceLimit,
        });
    }
    let decoded = adapters::decode_protocol(context.consumer, bytes, &context.cwd, protocol)?;
    crate::check_deadline(deadline)?;
    if let Operation::Outside(tool) = &decoded.operation {
        return Ok(Evaluation {
            outcome: Outcome::CoverageInsufficient {
                cause: CoverageGap::OutsideObservedTool { tool: tool.clone() },
                disposition: Disposition::ContinueLimitedPreflight,
                recovery: None,
            },
            coverage: Coverage::OutsideObservedToolCoverage { tool: tool.clone() },
            effects: Vec::new(),
        });
    }
    if context.require_execution_owner {
        let gap = CoverageGap::ExecutionOwnerUnavailable;
        return Ok(Evaluation {
            outcome: Outcome::CoverageInsufficient {
                cause: gap.clone(),
                disposition: Disposition::RequireVerifiedExecutionOwner,
                recovery: Some(recovery(context, &decoded.cwd, "execution_owner")),
            },
            coverage: Coverage::LimitedPreflight(vec![gap]),
            effects: Vec::new(),
        });
    }
    let catalog = load()?;
    let mut inspection = Inspection {
        context,
        probe,
        resolver: filesystem::Resolver::with_deadline(&context.home, &catalog, deadline),
        arm,
        gaps: Vec::new(),
        denial: None,
        appdata_reason: None,
        advice: Vec::new(),
        executable_qualifier: false,
        effects: Vec::new(),
        #[cfg(test)]
        source_entries: 0,
        #[cfg(test)]
        broad_root_queries: 0,
        deadline,
    };
    match &decoded.operation {
        Operation::Read(path) | Operation::Write(path) => inspection.target(
            &Target::new(
                filesystem::absolute_input(path, &decoded.cwd, &context.home),
                if matches!(decoded.operation, Operation::Write(_)) {
                    Effect::Write
                } else {
                    Effect::Read
                },
                Walk::None,
                Via::Tool,
            ),
            &decoded.cwd,
            EffectSource::Operand,
        )?,
        Operation::Search { root, glob } => {
            let root = if root.is_empty() { &decoded.cwd } else { root };
            let root = filesystem::absolute_input(root, &decoded.cwd, &context.home);
            let mut target = Target::new(root.clone(), Effect::Read, Walk::Visible, Via::Tool);
            target.search = true;
            inspection.target(&target, &decoded.cwd, EffectSource::Operand)?;
            if !glob.is_empty() && !glob.starts_with('!') {
                let mut target = Target::new(
                    format!("{}/{glob}", filesystem::literal_glob_root(&root)),
                    Effect::Read,
                    Walk::None,
                    Via::Tool,
                );
                target.glob = true;
                inspection.target(&target, &decoded.cwd, EffectSource::Operand)?;
            }
        }
        Operation::Shell(source) => {
            shell::check_nesting(source)?;
            inspection.shell(source, &decoded.cwd, 0)?;
        }
        Operation::Outside(_) => unreachable!(),
    }
    crate::check_deadline(deadline)?;
    let coverage = if inspection.gaps.is_empty() {
        Coverage::SupportedPreflight
    } else {
        Coverage::LimitedPreflight(inspection.gaps.clone())
    };
    let appdata = inspection.effects.iter().find(|effect| {
        matches!(
            effect,
            EffectRecord::ProtectedTarget {
                protection: Protection::AppData,
                ..
            }
        )
    });
    if let Some(EffectRecord::ProtectedTarget { write, source, .. }) = appdata {
        inspection.denial = Some(Reason {
            effect: match source {
                EffectSource::Cwd => "protected cwd: read protected App Data".into(),
                EffectSource::InlineCode => {
                    "CodeFile: inline interpreter token names protected App Data".into()
                }
                _ if *write => {
                    "write protected location; read protected App Data is excluded".into()
                }
                _ => "read protected App Data".into(),
            },
            rule: inspection.appdata_reason.unwrap_or(DenialRule::AppData),
        });
    } else if inspection.effects.contains(&EffectRecord::BroadRoot) {
        inspection.denial = Some(Reason {
            effect: format!(
                "broad recursive root reaches protected locations; HOME {} is excluded",
                context.home
            ),
            rule: DenialRule::Broad,
        });
    }
    if let Some(reason) = inspection.denial {
        let recovery = recovery(context, &decoded.cwd, &reason.effect);
        return Ok(Evaluation {
            outcome: Outcome::ProtectedDenial { reason, recovery },
            coverage,
            effects: inspection.effects,
        });
    }
    if let Some(cause) = inspection.gaps.iter().find(|g| {
        matches!(
            g,
            CoverageGap::ExecutorDivergence
                | CoverageGap::UnsupportedDialectConstruct
                | CoverageGap::UnsupportedShellSyntax
                | CoverageGap::IdentityBound
                | CoverageGap::InodeAlias
                | CoverageGap::InspectionBudget
        )
    }) {
        let mut recovery = recovery(context, &decoded.cwd, "unsupported");
        recovery.excluded_scope.push(
            match cause {
                CoverageGap::InodeAlias => "unresolved inode-addressed resource identity",
                CoverageGap::IdentityBound => {
                    "unresolved resource identity from bounded or cyclic alias traversal"
                }
                CoverageGap::InspectionBudget => "over-budget function expansion",
                CoverageGap::UnsupportedShellSyntax => {
                    "original unsupported shell syntax/control flow"
                }
                _ => "original unsupported executor/dialect constructs",
            }
            .into(),
        );
        if inspection.executable_qualifier {
            recovery.next_step = RecoveryStep::OwnerAction {description:"Replace the executable qualifier with explicit public names/results selected by the agent, then recheck through the same consumer. Task equivalence remains unverified.".into()};
            recovery
                .excluded_scope
                .push("Zsh executable qualifier".into());
        }
        return Ok(Evaluation {
            outcome: Outcome::CoverageInsufficient {
                cause: cause.clone(),
                disposition: Disposition::RejectUnsupportedSyntax,
                recovery: Some(recovery),
            },
            coverage,
            effects: inspection.effects,
        });
    }
    if let Some(cause) = inspection.gaps.first() {
        return Ok(Evaluation {
            outcome: Outcome::CoverageInsufficient {
                cause: cause.clone(),
                disposition: Disposition::ContinueLimitedPreflight,
                recovery: None,
            },
            coverage,
            effects: inspection.effects,
        });
    }
    let outcome = if inspection.advice.is_empty() {
        Outcome::NoObjection
    } else {
        Outcome::SoftAdvice(inspection.advice)
    };
    Ok(Evaluation {
        outcome,
        coverage,
        effects: inspection.effects,
    })
}

struct Inspection<'a> {
    context: &'a Context,
    probe: &'a mut dyn Probe,
    resolver: filesystem::Resolver<'a>,
    arm: Arm,
    gaps: Vec<CoverageGap>,
    denial: Option<Reason>,
    appdata_reason: Option<DenialRule>,
    advice: Vec<Advice>,
    executable_qualifier: bool,
    effects: Vec<EffectRecord>,
    #[cfg(test)]
    source_entries: usize,
    #[cfg(test)]
    broad_root_queries: usize,
    deadline: Option<std::time::Instant>,
}

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
    fn target(
        &mut self,
        target: &Target,
        cwd: &str,
        source: EffectSource,
    ) -> Result<(), CheckError> {
        crate::check_deadline(self.deadline)?;
        let mut target = target.clone();
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
                            format!("change protected {kind:?} resource")
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
                let resolved_broad = if let Some((lexical_path, matched)) = &lexical_broad
                    && lexical_path == &path
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
                        .may_traverse(&target, &resolved_home, self.probe)?
                {
                    self.hidden_search();
                }
            }
        }
        Ok(())
    }
    fn shell(&mut self, source: &str, cwd: &str, depth: usize) -> Result<(), CheckError> {
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
            let cwd = &command.cwd;
            let effects = targets::infer(
                &command,
                cwd,
                crate::record::HostFacts {
                    home: &self.context.home,
                    user: self.context.user.as_deref(),
                },
            );
            if let Some(pipeline) = command.pipeline {
                if effects.hidden_listing {
                    hidden_listings.insert(pipeline);
                }
                if effects.consumes_listing && hidden_listings.contains(&pipeline) {
                    self.effect(EffectRecord::HiddenContent);
                    self.denial.get_or_insert_with(|| Reason {
                        effect: "hidden listing with a content consumer reaches protected environment files".into(),
                        rule: DenialRule::HiddenSearch,
                    });
                }
            }
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
                let previous_denial = self.denial.take();
                for path in targets::code_paths(&code) {
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
            }
            for code in effects.code {
                self.shell(&code, cwd, depth + 1)?;
            }
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

fn recovery(context: &Context, cwd: &str, effect: &str) -> Recovery {
    let mut excluded_scope = vec![
        format!("{}/Library", context.home),
        format!("{}/.ssh", context.home),
        "protected environment-file and credential-file contents".into(),
    ];
    let description = if effect == "execution_owner" {
        excluded_scope = vec![
            "dynamic protected reads".into(),
            "out-of-domain writes/deletes".into(),
            "later interactive input".into(),
        ];
        "Establish and validate the named execution owner, then recheck. Hook-only preflight cannot enforce this domain."
    } else if effect.starts_with("broad recursive") {
        excluded_scope.insert(0, format!("HOME {} outside a separately verified public scope; a whole-HOME task remains incomplete", context.home));
        "The agent must select an explicit public root outside the excluded HOME scope and recheck its operation through the same consumer. A narrowed result does not complete the whole-HOME task."
    } else if effect == "extract protected environment dump" {
        excluded_scope = vec![
            "process environment dump".into(),
            "protected variable values".into(),
        ];
        "Ask the owner to inspect the needed setting without returning protected values, or select an explicit non-secret variable and recheck."
    } else if effect.starts_with("CodeFile:") {
        excluded_scope.push("execution of the denied inline interpreter".into());
        "Select a public input through a covered structured tool and recheck. Keep marker text as literal data when that is the task."
    } else {
        "Select an explicit public target or names-only operation outside the excluded scope, then recheck through the same consumer. The agent owns the continuation and its task equivalence."
    };
    Recovery {
        next_step: RecoveryStep::OwnerAction {
            description: description.into(),
        },
        preserved_scope: vec![format!("requested cwd: {cwd}")],
        excluded_scope,
        automatic_application_supported: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoProbe {
        calls: usize,
    }
    impl Probe for NoProbe {
        fn read_link(
            &mut self,
            _: &std::path::Path,
        ) -> std::io::Result<Option<std::path::PathBuf>> {
            self.calls += 1;
            Ok(None)
        }
        fn stat(&mut self, _: &std::path::Path) -> std::io::Result<Option<filesystem::Metadata>> {
            self.calls += 1;
            Ok(None)
        }
    }

    #[test]
    fn unchanged_resource_domains_share_broad_root_checks() {
        let context = Context {
            consumer: Consumer::Claude,
            home: "/h".into(),
            cwd: "/project".into(),
            user: None,
            zsh_executor: true,
            require_execution_owner: false,
            shell_observation_entries: std::cell::Cell::new(0),
        };
        let table = filesystem::FirmlinkTable::from_text("");
        for size in [8, 16, 32] {
            let mut probe = NoProbe { calls: 0 };
            let mut inspection = Inspection {
                context: &context,
                probe: &mut probe,
                resolver: filesystem::Resolver::new(&context.home, &table),
                arm: Arm::Brush,
                gaps: Vec::new(),
                denial: None,
                appdata_reason: None,
                advice: Vec::new(),
                executable_qualifier: false,
                effects: Vec::new(),
                source_entries: 0,
                broad_root_queries: 0,
                deadline: None,
            };
            inspection
                .shell(&"ssh host public*;".repeat(size), &context.cwd, 0)
                .unwrap();
            assert_eq!(inspection.broad_root_queries, size, "size={size}");
            assert!(inspection.denial.is_none() && inspection.gaps.is_empty());
        }
    }

    #[test]
    fn inspection_recursion_frontier_is_independent_of_delimiter_depth() {
        let context = Context {
            consumer: Consumer::Claude,
            home: "/h".into(),
            cwd: "/p".into(),
            user: None,
            zsh_executor: true,
            require_execution_owner: false,
            shell_observation_entries: std::cell::Cell::new(0),
        };
        let table = filesystem::FirmlinkTable::from_text("");
        let mut probe = NoProbe { calls: 0 };
        let mut inspection = Inspection {
            context: &context,
            probe: &mut probe,
            resolver: filesystem::Resolver::new(&context.home, &table),
            arm: Arm::Brush,
            gaps: Vec::new(),
            denial: None,
            appdata_reason: None,
            advice: Vec::new(),
            executable_qualifier: false,
            effects: Vec::new(),
            source_entries: 0,
            broad_root_queries: 0,
            deadline: None,
        };
        inspection.shell("true", &context.cwd, 64).unwrap();
        assert_eq!(context.shell_observation_entries.get(), 1);
        assert_eq!(
            inspection.shell("true", &context.cwd, 65).unwrap_err().kind,
            CheckErrorKind::ResourceLimit
        );
        inspection.shell("sh -c true", &context.cwd, 64).unwrap();
        assert_eq!(context.shell_observation_entries.get(), 2);
    }

    #[test]
    fn literal_eval_bodies_are_observed_once_in_their_scope() {
        let context = Context {
            consumer: Consumer::Claude,
            home: "/h".into(),
            user: None,
            cwd: "/project".into(),
            zsh_executor: true,
            require_execution_owner: false,
            shell_observation_entries: std::cell::Cell::new(0),
        };
        for n in [2, 4, 8, 16, 24] {
            let mut probe = NoProbe { calls: 0 };
            let catalog = filesystem::FirmlinkTable::from_text("");
            let mut inspection = Inspection {
                context: &context,
                probe: &mut probe,
                resolver: filesystem::Resolver::new(&context.home, &catalog),
                arm: Arm::Brush,
                gaps: Vec::new(),
                denial: None,
                appdata_reason: None,
                advice: Vec::new(),
                executable_qualifier: false,
                effects: Vec::new(),
                source_entries: 0,
                broad_root_queries: 0,
                deadline: None,
            };
            inspection
                .shell(&format!("{}cat .env", "eval ".repeat(n)), &context.cwd, 0)
                .unwrap();
            assert_eq!(
                inspection.denial.as_ref().unwrap().rule,
                crate::DenialRule::File
            );
            assert!(inspection.effects.contains(&EffectRecord::ProtectedTarget {
                protection: filesystem::Protection::Environment,
                write: false,
                source: EffectSource::Nested,
            }));
            assert!(inspection.gaps.is_empty());
            assert!(
                inspection.source_entries <= n + 1,
                "eval depth {n}: {} observations",
                inspection.source_entries
            );
        }
    }

    #[test]
    fn catalog_initialization_fault_blocks_supported_request() {
        let context = Context {
            consumer: Consumer::Claude,
            home: "/h".into(),
            user: None,
            cwd: "/project".into(),
            zsh_executor: true,
            require_execution_owner: false,
            shell_observation_entries: std::cell::Cell::new(0),
        };
        let mut probe = NoProbe { calls: 0 };
        let loads = std::cell::Cell::new(0);
        let result = evaluate_with_catalog_loader(
            Event {
                bytes: br#"{"tool_name":"Read","tool_input":{"file_path":"public"}}"#,
                context: &context,
                probe: &mut probe,
            },
            Arm::Brush,
            || {
                loads.set(loads.get() + 1);
                Err(CheckError {
                    kind: CheckErrorKind::ProbeFault,
                })
            },
            None,
            adapters::Protocol::Tool,
        );
        assert!(matches!(
            result,
            Err(CheckError {
                kind: CheckErrorKind::ProbeFault
            })
        ));
        let wire = adapters::render(context.consumer, &result);
        assert_eq!(wire.exit, 2);
        assert!(wire.stdout.is_empty() && wire.stderr.contains("could not complete this check"));
        assert_eq!(probe.calls, 0);
        assert_eq!(loads.get(), 1);
        assert_eq!(context.shell_observation_entries.get(), 0);
    }
}
