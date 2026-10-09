use crate::{
    Advice, CheckError, CheckErrorKind, Coverage, CoverageGap, DenialRule, Disposition,
    EffectRecord, EffectSource, Evaluation, Outcome, Reason, Recovery, RecoveryStep,
    adapters::{self, Consumer, Operation},
    filesystem::{self, Probe, Protection},
    limits::MAX_INPUT_BYTES,
    record::{Effect, Via, Walk},
    shell::{self, Arm},
    targets::Target,
};

mod inspection;
mod messages;
pub(crate) use messages::refusal_message;

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
    inspect_operation(&mut inspection, &decoded)?;
    crate::check_deadline(deadline)?;
    Ok(finish_inspection(inspection, &decoded.cwd))
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

fn inspect_operation(
    inspection: &mut Inspection<'_>,
    decoded: &adapters::CanonicalEvent,
) -> Result<(), CheckError> {
    match &decoded.operation {
        Operation::Read(path) | Operation::Write(path) => inspection.target(
            &Target::new(
                filesystem::absolute_input(path, &decoded.cwd, &inspection.context.home),
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
            let root = filesystem::absolute_input(root, &decoded.cwd, &inspection.context.home);
            let mut target = Target::new(root.clone(), Effect::Read, Walk::Visible, Via::Tool);
            target.search = true;
            inspection.target(&target, &decoded.cwd, EffectSource::Operand)?;
            if !glob.is_empty() && !glob.starts_with('!') {
                let mut target = Target::new(
                    filesystem::grep_pattern(&root, glob),
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
    Ok(())
}

fn finish_inspection(mut inspection: Inspection<'_>, cwd: &str) -> Evaluation {
    let context = inspection.context;
    let coverage = if inspection.gaps.is_empty() {
        Coverage::SupportedPreflight
    } else {
        Coverage::LimitedPreflight(inspection.gaps.clone())
    };
    prioritize_protection(&mut inspection);
    if let Some(reason) = inspection.denial {
        let recovery = recovery(context, cwd, &reason.effect);
        return Evaluation {
            outcome: Outcome::ProtectedDenial { reason, recovery },
            coverage,
            effects: inspection.effects,
        };
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
        let mut recovery = recovery(context, cwd, "unsupported");
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
        return Evaluation {
            outcome: Outcome::CoverageInsufficient {
                cause: cause.clone(),
                disposition: Disposition::RejectUnsupportedSyntax,
                recovery: Some(recovery),
            },
            coverage,
            effects: inspection.effects,
        };
    }
    if let Some(cause) = inspection.gaps.first() {
        return Evaluation {
            outcome: Outcome::CoverageInsufficient {
                cause: cause.clone(),
                disposition: Disposition::ContinueLimitedPreflight,
                recovery: None,
            },
            coverage,
            effects: inspection.effects,
        };
    }
    let outcome = if inspection.advice.is_empty() {
        Outcome::NoObjection
    } else {
        Outcome::SoftAdvice(inspection.advice)
    };
    Evaluation {
        outcome,
        coverage,
        effects: inspection.effects,
    }
}

fn prioritize_protection(inspection: &mut Inspection<'_>) {
    let context = inspection.context;
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
}

#[cfg(test)]
mod tests;
