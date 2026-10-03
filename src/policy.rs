use crate::{
    Advice, CheckError, CheckErrorKind, Coverage, CoverageGap, Disposition, Evaluation, Outcome,
    Reason, Recovery, RecoveryStep,
    adapters::{self, Consumer, Operation, PublicTask},
    filesystem::{self, Identity, Probe, Protection},
    limits::MAX_INPUT_BYTES,
    shell::{self, Arm},
    targets::{self, Target},
};

/// Trusted offline context; tool-input fields cannot override the public continuation.
pub struct Context {
    pub consumer: Consumer,
    pub home: String,
    pub cwd: String,
    pub project: String,
    pub objective: String,
    pub public_task: PublicTask,
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
        });
    }
    if context.require_execution_owner {
        let gap = CoverageGap::ExecutionOwnerUnavailable;
        return Ok(Evaluation {
            outcome: Outcome::CoverageInsufficient {
                cause: gap.clone(),
                disposition: Disposition::RequireVerifiedExecutionOwner,
                recovery: Some(recovery(context, "execution_owner", &context.public_task)),
            },
            coverage: Coverage::LimitedPreflight(vec![gap]),
        });
    }
    let mut inspection = Inspection {
        context,
        probe,
        arm,
        gaps: Vec::new(),
        denial: None,
        advice: false,
        search: None,
        continuation: None,
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
                )?;
            }
            inspection.search = Some(PublicTask::Search {
                pattern: decoded
                    .input
                    .get("pattern")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                glob: glob.clone(),
            });
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
    if let Some(effect) = inspection.denial {
        let task = if effect.starts_with("broad recursive") {
            inspection
                .search
                .as_ref()
                .or(inspection.continuation.as_ref())
                .unwrap_or(&context.public_task)
        } else if effect.starts_with("CodeFile:") {
            &context.public_task
        } else {
            inspection
                .continuation
                .as_ref()
                .unwrap_or(&context.public_task)
        };
        let mut recovery = recovery(context, &effect, task);
        if matches!(decoded.operation, Operation::Write(_)) {
            recovery.next_step = adapters::write_recovery(
                context.consumer,
                &context.project,
                &context.public_task,
                &decoded,
            );
        }
        return Ok(Evaluation {
            outcome: Outcome::ProtectedDenial {
                reason: Reason { effect },
                recovery,
            },
            coverage,
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
        let mut recovery = recovery(context, "unsupported", &context.public_task);
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
        if matches!(&decoded.operation,Operation::Shell(source) if source.contains("(e:") || source.contains("(+"))
        {
            recovery.next_step=RecoveryStep::OwnerAction {description:"Replace executable qualifier with explicit public names/results matching this task; recheck through the same consumer.".into()};
            recovery.excluded_scope = vec!["Zsh executable qualifier".into()];
            recovery
                .objective
                .push_str(" remains incomplete until equivalence and result are demonstrated");
        }
        return Ok(Evaluation {
            outcome: Outcome::CoverageInsufficient {
                cause: cause.clone(),
                disposition: Disposition::RejectUnsupportedSyntax,
                recovery: Some(recovery),
            },
            coverage,
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
        });
    }
    let outcome = if inspection.advice && context.consumer == Consumer::Claude {
        Outcome::SoftAdvice(vec![Advice {message:"-r replaces matching text; it is not recursive search. Use an explicit project root and -n when line numbers are intended.".into()}])
    } else {
        Outcome::NoObjection
    };
    Ok(Evaluation { outcome, coverage })
}

struct Inspection<'a> {
    context: &'a Context,
    probe: &'a mut dyn Probe,
    arm: Arm,
    gaps: Vec<CoverageGap>,
    denial: Option<String>,
    advice: bool,
    search: Option<PublicTask>,
    continuation: Option<PublicTask>,
}

impl Inspection<'_> {
    fn gap(&mut self, value: CoverageGap) {
        if !self.gaps.contains(&value) {
            self.gaps.push(value);
        }
    }
    fn target(&mut self, target: &Target, cwd: &str) -> Result<(), CheckError> {
        match filesystem::identify(&target.path, cwd, &self.context.home, self.probe)? {
            Identity::Protected(kind) => {
                let touches = kind == Protection::AppData
                    || kind == Protection::SshPrivate
                    || !target.write && !target.name_only;
                if touches {
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
                if target.recursive
                    && (path == "/"
                        || path == resolved_home
                        || path == format!("{resolved_home}/Library"))
                {
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
                    self.denial.get_or_insert_with(||"hidden listing with a content consumer reaches protected environment files".into());
                }
            }
            for value in effects.gaps {
                self.gap(value);
            }
            if effects.dump {
                if self.denial.is_none() {
                    self.continuation = Some(PublicTask::HomeSetting);
                }
                self.denial
                    .get_or_insert_with(|| "extract protected environment dump".into());
            }
            if effects.variable {
                self.denial
                    .get_or_insert_with(|| "extract protected credential variable".into());
            }
            if effects.hidden_content {
                self.denial.get_or_insert_with(|| {
                    "hidden recursive content search reaches protected environment files".into()
                });
            }
            self.advice |= effects.replace_advice;
            if self.denial.is_none() && command.argv.first().is_some_and(|s| s == "ls") {
                self.continuation = Some(PublicTask::List {
                    path: self.context.project.clone(),
                });
            }
            if let Some(search) = effects.search {
                self.search.get_or_insert(PublicTask::Search {
                    pattern: search.pattern,
                    glob: search.glob.unwrap_or_default(),
                });
            }
            for target in effects.targets {
                self.target(&target, cwd)?;
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

fn recovery(context: &Context, effect: &str, task: &PublicTask) -> Recovery {
    let mut objective = context.objective.clone();
    let mut excluded_scope = vec![
        format!("{}/Library", context.home),
        format!("{}/.ssh", context.home),
        format!("{}/.env", context.project),
    ];
    let next_step = if effect == "execution_owner" {
        excluded_scope = vec![
            "dynamic protected reads".into(),
            "out-of-domain writes/deletes".into(),
            "later interactive input".into(),
        ];
        objective
            .push_str(" remains incomplete; independent known-public work may complete separately");
        RecoveryStep::OwnerAction {description:"Establish and validate the named execution owner before promising this domain; recheck afterward. Hook-only preflight cannot establish that guarantee.".into()}
    } else {
        if effect.starts_with("broad recursive") {
            excluded_scope.insert(0, format!("{} outside {}", context.home, context.project));
            objective.push_str("; only the project-scoped result may complete, a whole-HOME task remains incomplete");
        }
        if effect == "extract protected environment dump" {
            excluded_scope = vec![
                "process environment dump".into(),
                "protected variable values".into(),
            ];
        }
        if effect.starts_with("CodeFile:") {
            excluded_scope = vec![
                "protected file contents".into(),
                "execution of the denied inline interpreter".into(),
            ];
        }
        adapters::public_recovery(context.consumer, &context.project, task)
    };
    Recovery {
        next_step,
        objective,
        preserved_scope: vec![context.project.clone()],
        excluded_scope,
        automatic_application_supported: false,
    }
}
