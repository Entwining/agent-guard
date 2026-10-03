use crate::{
    Advice, CheckError, CheckErrorKind, Coverage, CoverageGap, Disposition, EffectRecord,
    EffectSource, Evaluation, Outcome, Reason, Recovery, RecoveryStep,
    adapters::{self, Consumer, Operation},
    filesystem::{self, Identity, Probe, Protection},
    limits::MAX_INPUT_BYTES,
    shell::{self, Arm},
    targets::{self, Target},
};

/// Consumer and host facts. Task intent and agent continuations are not guard inputs.
pub struct Context {
    pub consumer: Consumer,
    pub home: String,
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
    let mut inspection = Inspection {
        context,
        probe,
        arm,
        gaps: Vec::new(),
        denial: None,
        advice: false,
        executable_qualifier: false,
        effects: Vec::new(),
    };
    match &decoded.operation {
        Operation::Read(path) | Operation::Write(path) => inspection.target(
            &Target {
                path: path.clone(),
                recursive: false,
                write: matches!(decoded.operation, Operation::Write(_)),
                name_only: false,
            },
            &decoded.cwd,
            EffectSource::Operand,
        )?,
        Operation::Search { root, glob } => {
            let root = if root.is_empty() { &decoded.cwd } else { root };
            inspection.target(
                &Target {
                    path: root.clone(),
                    write: false,
                    recursive: true,
                    name_only: false,
                },
                &decoded.cwd,
                EffectSource::Operand,
            )?;
            if !glob.is_empty() && !glob.starts_with('!') {
                inspection.target(
                    &Target {
                        path: format!("{root}/{}", glob.rsplit('/').next().unwrap_or(glob)),
                        write: false,
                        recursive: false,
                        name_only: false,
                    },
                    &decoded.cwd,
                    EffectSource::Operand,
                )?;
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
    let outcome = if inspection.advice {
        Outcome::SoftAdvice(vec![Advice {message:"-r replaces matching text; it is not recursive search. Use an explicit project root and -n when line numbers are intended.".into()}])
    } else {
        Outcome::NoObjection
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
    arm: Arm,
    gaps: Vec<CoverageGap>,
    denial: Option<String>,
    advice: bool,
    executable_qualifier: bool,
    effects: Vec<EffectRecord>,
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
        match filesystem::identify_scope(
            &target.path,
            cwd,
            &self.context.home,
            target.recursive,
            self.probe,
        )? {
            Identity::Protected(kind) => {
                let touches = kind == Protection::AppData
                    || kind == Protection::SshPrivate
                    || !target.write && !target.name_only;
                if touches {
                    self.effect(EffectRecord::ProtectedTarget {
                        protection: kind,
                        write: target.write,
                        source,
                    });
                    self.denial.get_or_insert_with(|| {
                        if target.write {
                            format!("write protected location; {} is excluded", kind.effect())
                        } else {
                            kind.effect().to_owned()
                        }
                    });
                }
            }
            Identity::Bound => self.gap(CoverageGap::IdentityBound),
            Identity::Public(path) => {
                let resolved_home = match filesystem::identify(
                    &self.context.home,
                    &self.context.home,
                    &self.context.home,
                    self.probe,
                )? {
                    Identity::Public(path) => path,
                    Identity::Bound => {
                        self.gap(CoverageGap::IdentityBound);
                        return Ok(());
                    }
                    Identity::Protected(_) => self.context.home.clone(),
                };
                if target.recursive && filesystem::broad_root(&path, &resolved_home) {
                    self.effect(EffectRecord::BroadRoot);
                    self.denial.get_or_insert_with(|| {
                        format!(
                            "broad recursive root reaches protected locations; HOME {} is excluded",
                            self.context.home
                        )
                    });
                }
                if target.recursive
                    && !target.name_only
                    && ["/.docker", "/.kube", "/.cargo", "/.config"]
                        .iter()
                        .any(|suffix| path.ends_with(suffix))
                {
                    self.effect(EffectRecord::HiddenContent);
                    self.denial.get_or_insert_with(|| {
                        "recursive search reaches protected credential-file contents".into()
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
        match filesystem::identify(cwd, cwd, &self.context.home, self.probe)? {
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
        let observation = shell::observe(
            source,
            self.arm,
            &self.context.home,
            cwd,
            self.context.zsh_executor,
        )?;
        self.executable_qualifier |= observation.executable_qualifier;
        for value in observation.gaps {
            self.gap(value);
        }
        let mut hidden_listings = std::collections::BTreeSet::new();
        for command in observation.commands {
            let cwd = &command.cwd;
            let effects = targets::infer(&command, cwd);
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
                self.gap(value);
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
            if effects.hidden_content {
                self.effect(EffectRecord::HiddenContent);
                self.denial.get_or_insert_with(|| {
                    "hidden recursive content search reaches protected environment files".into()
                });
            }
            self.advice |= effects.replace_advice;
            for target in effects.targets {
                self.target(
                    &target,
                    cwd,
                    if depth > 0 || command.nested {
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
                        &Target {
                            path,
                            write: false,
                            recursive: false,
                            name_only: false,
                        },
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
