//! Offline preflight trial. No production hook or execution-confinement path uses this package.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::fmt;

pub mod adapters;
pub mod filesystem;
pub mod limits;
mod policy;
pub mod shell;
mod targets;

pub use policy::{Context, Event, evaluate, evaluate_with_arm};

/// A completed preflight decision retains coverage independently of whether the call proceeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    pub outcome: Outcome,
    pub coverage: Coverage,
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
pub struct Advice {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason {
    pub effect: String,
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
    MalformedInput,
    InputFailure,
    GuardFault,
    ProbeFault,
    ResourceLimit,
    Deadline,
    Cancelled,
    BrokenEnrollment,
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
