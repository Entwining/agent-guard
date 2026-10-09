//! Preflight evaluation for the native guard; operating system confinement belongs to the consumer.

#![cfg_attr(not(test), deny(clippy::unwrap_used))]

use std::fmt;

pub mod adapters;
pub mod entry;
pub mod filesystem;
pub mod limits;
mod policy;
pub mod record;
pub mod shell;
mod targets;

pub use policy::{
    Context, Event, evaluate, evaluate_with_arm, evaluate_with_catalog, evaluate_with_deadline,
};

pub(crate) fn check_deadline(deadline: Option<std::time::Instant>) -> Result<(), CheckError> {
    if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
        Err(CheckError {
            kind: CheckErrorKind::Deadline,
        })
    } else {
        Ok(())
    }
}

/// A completed preflight decision retains coverage independently of whether the call proceeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    pub outcome: Outcome,
    pub coverage: Coverage,
    pub effects: Vec<EffectRecord>,
}

/// Static effects identified by their owner; these are not completed I/O receipts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectRecord {
    ProtectedTarget {
        protection: filesystem::Protection,
        write: bool,
        source: EffectSource,
    },
    BroadRoot,
    EnvironmentDump,
    CredentialVariable,
    HostingToken,
    Keychain,
    StoredSecret,
    NetworkTrace,
    HiddenContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectSource {
    Operand,
    Nested,
    InlineCode,
    Cwd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    NoObjection,
    SoftAdvice(Vec<Advice>),
    ProtectedDenial {
        reason: Reason,
        recovery: Recovery,
    },
    CoverageInsufficient {
        cause: CoverageGap,
        disposition: Disposition,
        recovery: Option<Recovery>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Advice {
    RgReplace,
    RgInclude,
    RgBre,
}

impl Advice {
    pub fn message(&self) -> &'static str {
        match self {
            Self::RgReplace => {
                "rg -r means --replace. Drop -r; use -n for line numbers, or spell --replace VALUE for an intentional replacement."
            }
            Self::RgInclude => {
                "rg has no --include flag. Filter files with -g GLOB (for example -g '*.ts') or a type filter such as -t ts."
            }
            Self::RgBre => {
                "rg regex is not grep BRE: a\\|b matches a literal pipe. Write alternation as a|b; for a literal pipe, use [|] or -F."
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason {
    pub effect: String,
    pub rule: DenialRule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenialRule {
    ResourceChange,
    AppData,
    Broad,
    File,
    CodeFile,
    HiddenSearch,
    Dump,
    Variable,
    Token,
    Keychain,
    StoredSecret,
    Trace,
    Upload,
    Ssh,
    GrepSsh,
}

/// Event scope survives rendering; choosing and proving a continuation belongs to the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovery {
    pub next_step: RecoveryStep,
    pub preserved_scope: Vec<String>,
    pub excluded_scope: Vec<String>,
    pub automatic_application_supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryStep {
    RecheckOperation {
        description: String,
    },
    OwnerAction {
        description: String,
    },
    StructuredOperation {
        tool: String,
        input: serde_json::Value,
        cwd: String,
        description: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    ContinueLimitedPreflight,
    RejectUnsupportedSyntax,
    RequireVerifiedExecutionOwner,
}

/// Supported means supported preflight observation, never arbitrary-code confinement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Coverage {
    SupportedPreflight,
    LimitedPreflight(Vec<CoverageGap>),
    OutsideObservedToolCoverage { tool: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverageGap {
    InodeAlias,
    InputByteLimit,
    NestingLimit,
    AbsoluteCwdRequired,
    InvalidEncoding,
    UnknownProgram { program: String },
    UnresolvedTarget,
    UnsupportedShellSyntax,
    ExecutorDivergence,
    OutsideObservedTool { tool: String },
    ExecutionOwnerUnavailable,
    IdentityBound,
    InspectionBudget,
    InterpreterChosenRead,
    UnsupportedDialectConstruct,
}

/// Entry owners must block on this error; it is not a completed no-objection decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckError {
    pub kind: CheckErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckErrorKind {
    RelativeCwd,
    InvalidEncoding,
    MalformedInput,
    InputFailure,
    GuardFault,
    ProbeFault,
    ResourceLimit,
    Deadline,
    Cancelled,
    BrokenEnrollment,
}

impl CheckError {
    pub(crate) fn consumer_message(self) -> &'static str {
        match self.kind {
            CheckErrorKind::Deadline => {
                "The agent guard could not complete this check before its deadline, so the call is blocked. Split the work into smaller calls naming explicit public targets, then recheck each call."
            }
            CheckErrorKind::ResourceLimit => {
                "The agent guard could not complete this check within its resource limit, so the call is blocked. Split the work into smaller calls naming explicit public targets, then recheck each call."
            }
            CheckErrorKind::RelativeCwd => {
                "The agent guard could not complete this check, so the call is blocked. Supply an absolute cwd in the event, then recheck the call."
            }
            CheckErrorKind::InvalidEncoding
            | CheckErrorKind::MalformedInput
            | CheckErrorKind::InputFailure => {
                "The agent guard could not complete this check, so the call is blocked. Send a complete UTF-8 JSON event with the documented tool fields and an absolute cwd, then recheck the call."
            }
            _ => {
                "The agent guard could not complete this check, so the call is blocked. Ask the checker owner to run `agent-guard --version` and check a single public file, repair the reported fault, then recheck the call before running it."
            }
        }
    }
}

impl From<serde_json::Error> for CheckError {
    fn from(error: serde_json::Error) -> Self {
        let kind = match error.classify() {
            serde_json::error::Category::Io => CheckErrorKind::InputFailure,
            _ => CheckErrorKind::MalformedInput,
        };
        Self { kind }
    }
}

impl fmt::Display for CheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // JSON error messages can contain submitted values; render only the failure category.
        let message = match self.kind {
            CheckErrorKind::RelativeCwd => "event cwd is not absolute",
            CheckErrorKind::InvalidEncoding => "event input is not UTF-8",
            CheckErrorKind::MalformedInput => "invalid event input",
            CheckErrorKind::InputFailure => "event input could not be read",
            CheckErrorKind::GuardFault => "checker failed",
            CheckErrorKind::ProbeFault => "filesystem probe failed",
            CheckErrorKind::ResourceLimit => "checker resource limit exceeded",
            CheckErrorKind::Deadline => "checker deadline exceeded",
            CheckErrorKind::Cancelled => "check cancelled",
            CheckErrorKind::BrokenEnrollment => "consumer enrollment is unavailable",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for CheckError {}
