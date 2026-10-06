use crate::{
    Advice, CheckError, CheckErrorKind, Coverage, CoverageGap, Disposition, EffectRecord,
    EffectSource, Evaluation, Outcome, Reason, Recovery, RecoveryStep,
    adapters::{self, Consumer, Operation},
    filesystem::{self, Identity, Probe, Protection},
    limits::MAX_INPUT_BYTES,
    record::{Effect, Via, Walk},
    shell::{self, Arm},
    targets::{self, Target},
};

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
    evaluate_with_catalog_loader(event, arm, filesystem::FirmlinkTable::load)
}

/// Typed host metadata for isolated evaluations; hook request input has no catalog field.
pub fn evaluate_with_catalog(
    event: Event<'_>,
    arm: Arm,
    catalog: filesystem::FirmlinkTable,
) -> Result<Evaluation, CheckError> {
    evaluate_with_catalog_loader(event, arm, || Ok(catalog))
}

fn evaluate_with_catalog_loader(
    event: Event<'_>,
    arm: Arm,
    load: impl FnOnce() -> Result<filesystem::FirmlinkTable, CheckError>,
) -> Result<Evaluation, CheckError> {
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
    let decoded = adapters::decode(context.consumer, bytes, &context.cwd)?;
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
        resolver: filesystem::Resolver::new(&context.home, &catalog),
        arm,
        gaps: Vec::new(),
        denial: None,
        advice: Vec::new(),
        executable_qualifier: false,
        effects: Vec::new(),
        #[cfg(test)]
        source_entries: 0,
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
                    format!("{root}/{}", glob.rsplit('/').next().unwrap_or(glob)),
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
        inspection.denial = Some(match source {
            EffectSource::Cwd => "protected cwd: read protected App Data".into(),
            EffectSource::InlineCode => {
                "CodeFile: inline interpreter token names protected App Data".into()
            }
            _ if *write => "write protected location; read protected App Data is excluded".into(),
            _ => "read protected App Data".into(),
        });
    } else if inspection.effects.contains(&EffectRecord::BroadRoot) {
        inspection.denial = Some(format!(
            "broad recursive root reaches protected locations; HOME {} is excluded",
            context.home
        ));
    }
    if let Some(effect) = inspection.denial {
        let recovery = recovery(context, &decoded.cwd, &effect);
        return Ok(Evaluation {
            outcome: Outcome::ProtectedDenial {
                reason: Reason { effect },
                recovery,
            },
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
                | CoverageGap::InspectionBudget
        )
    }) {
        let mut recovery = recovery(context, &decoded.cwd, "unsupported");
        recovery.excluded_scope.push(
            match cause {
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
    denial: Option<String>,
    advice: Vec<Advice>,
    executable_qualifier: bool,
    effects: Vec<EffectRecord>,
    #[cfg(test)]
    source_entries: usize,
}

impl Inspection<'_> {
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
        let mut target = target.clone();
        // Broad traversal is independent of a narrower credential identity.
        // Decide a lexical root before probes, then check resolved aliases below.
        let broad_access = target.via != Via::Items
            && !(target.via == Via::Tool && target.glob)
            && (target.walk != Walk::None || target.glob)
            && (target.effect != Effect::Name || target.glob);
        if broad_access && filesystem::broad_root(&target.path, &self.context.home, target.glob) {
            self.effect(EffectRecord::BroadRoot);
        }
        // A literal relative ~/ prefix stays anchored at cwd, including with a
        // runtime-derived suffix. Its link aliases still need normal resolution.
        let relative_tilde = target.unresolved.starts_with(&format!("{cwd}/~/"));
        let identity = if target.via == Via::Items {
            let Some(kind) = (target.effect == Effect::Read)
                .then(|| filesystem::credential_read(&target.path, &self.context.home, target.glob))
                .flatten()
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
                        matches!(target.effect, Effect::Read | Effect::Write | Effect::List)
                    }
                    _ => target.effect == Effect::Read,
                };
                if touches {
                    self.effect(EffectRecord::ProtectedTarget {
                        protection: kind,
                        write: target.effect == Effect::Write,
                        source,
                    });
                    self.denial.get_or_insert_with(|| {
                        if target.effect == Effect::Write {
                            format!("write protected location; {} is excluded", kind.effect())
                        } else {
                            kind.effect().to_owned()
                        }
                    });
                }
            }
            Identity::Bound => self.gap(CoverageGap::IdentityBound),
            Identity::Public(path) => {
                let resolved_home = match self.resolver.home(self.probe)? {
                    Identity::Public(path) => path,
                    Identity::Bound => {
                        self.gap(CoverageGap::IdentityBound);
                        return Ok(());
                    }
                    Identity::Protected(_) => self.context.home.clone(),
                };
                if broad_access && filesystem::broad_root(&path, &resolved_home, target.glob) {
                    self.effect(EffectRecord::BroadRoot);
                    self.denial.get_or_insert_with(|| {
                        format!(
                            "broad recursive root reaches protected locations; HOME {} is excluded",
                            self.context.home
                        )
                    });
                }
            }
        }
        Ok(())
    }
    fn shell(&mut self, source: &str, cwd: &str, depth: usize) -> Result<(), CheckError> {
        if depth > crate::limits::MAX_NESTING {
            return Err(CheckError {
                kind: CheckErrorKind::ResourceLimit,
            });
        }
        let mut target = Target::new(cwd.to_owned(), Effect::Read, Walk::None, Via::Cwd);
        match self.resolver.target(&mut target, cwd, self.probe)? {
            Identity::Protected(kind) => {
                self.effect(EffectRecord::ProtectedTarget {
                    protection: kind,
                    write: false,
                    source: EffectSource::Cwd,
                });
                self.denial
                    .get_or_insert_with(|| format!("protected cwd: {}", kind.effect()));
            }
            Identity::Bound => self.gap(CoverageGap::IdentityBound),
            Identity::Public(_) => {}
        }
        self.context
            .shell_observation_entries
            .set(self.context.shell_observation_entries.get() + 1);
        let observation = shell::observe_with_user(
            source,
            self.arm,
            &self.context.home,
            cwd,
            self.context.user.as_deref(),
            self.context.zsh_executor,
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
                    self.denial.get_or_insert_with(||"hidden listing with a content consumer reaches protected environment files".into());
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
                self.denial
                    .get_or_insert_with(|| "extract protected environment dump".into());
            }
            if effects.variable {
                self.effect(EffectRecord::CredentialVariable);
                self.denial
                    .get_or_insert_with(|| "extract protected credential variable".into());
            }
            if effects.token {
                self.effect(EffectRecord::HostingToken);
                self.denial.get_or_insert_with(|| {
                    "This prints a Git hosting token. Use auth status without token-display flags; if authentication needs repair, ask the user to update the credential in their terminal.".into()
                });
            }
            if effects.keychain {
                self.effect(EffectRecord::Keychain);
                self.denial.get_or_insert_with(|| "extract a password from the macOS Keychain; run the authorized client that consumes it without printing it".into());
            }
            if effects.stored_secret {
                self.effect(EffectRecord::StoredSecret);
                self.denial.get_or_insert_with(|| "This prints a stored secret or access token. Run the command that uses the credential without printing it, or ask the user to run it in their own terminal and share only the non-secret fact needed.".into());
            }
            if effects.trace {
                self.effect(EffectRecord::NetworkTrace);
                self.denial.get_or_insert_with(|| "curl verbose or trace output can print authentication headers; drop -v and --trace and use a normal request".into());
            }
            if effects.hidden_content {
                self.effect(EffectRecord::HiddenContent);
                self.denial.get_or_insert_with(|| {
                    "hidden recursive content search reaches protected environment files".into()
                });
            }
            // native/rules/workflow.go:13-39 owns usage advice; D22 retains
            // the trial's replacement wording and consumer-specific rendering.
            for (applies, message) in [
                (
                    effects.replace_advice,
                    "-r replaces matching text; it is not recursive search. Use an explicit project root and -n when line numbers are intended.",
                ),
                (
                    effects.include_advice,
                    "rg has no --include flag. Filter files with -g GLOB (for example -g '*.ts') or a type filter such as -t ts.",
                ),
                (
                    effects.bre_advice,
                    "rg regex is not grep BRE: a\\|b matches a literal pipe. Write alternation as a|b; for a literal pipe, use [|] or -F.",
                ),
            ] {
                if applies && !self.advice.iter().any(|advice| advice.message == message) {
                    self.advice.push(Advice {
                        message: message.into(),
                    });
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
                        *reason = format!(
                            "CodeFile: inline interpreter token names a protected path; {reason} is excluded"
                        );
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
                advice: Vec::new(),
                executable_qualifier: false,
                effects: Vec::new(),
                source_entries: 0,
            };
            inspection
                .shell(&format!("{}true", "eval ".repeat(n)), &context.cwd, 0)
                .unwrap();
            assert!(inspection.denial.is_none());
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
        );
        assert!(matches!(
            result,
            Err(CheckError {
                kind: CheckErrorKind::ProbeFault
            })
        ));
        let wire = adapters::render(context.consumer, &result);
        assert_eq!(wire.exit, 2);
        assert!(wire.stdout.is_empty() && wire.stderr.contains("filesystem probe failed"));
        assert_eq!(probe.calls, 0);
        assert_eq!(loads.get(), 1);
        assert_eq!(context.shell_observation_entries.get(), 0);
    }
}
