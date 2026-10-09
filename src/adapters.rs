use crate::{
    CheckError, CheckErrorKind, Disposition, EffectRecord, Evaluation, Outcome, Recovery,
    RecoveryStep,
};
use serde_json::{Value, json};

pub fn effects_value(effects: &[EffectRecord]) -> Value {
    json!(effects.iter().map(|effect| match effect {
        EffectRecord::ProtectedTarget { protection, write, source } => json!({"kind":"ProtectedTarget","protection":format!("{protection:?}"),"write":write,"source":format!("{source:?}")}),
        other => json!({"kind":format!("{other:?}")}),
    }).collect::<Vec<_>>())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consumer {
    Claude,
    Codex,
    Pi,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Shell(String),
    Read(String),
    Write(String),
    Search { root: String, glob: String },
    Outside(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalEvent {
    pub operation: Operation,
    pub cwd: String,
    pub tool: String,
    pub input: Value,
}

fn malformed() -> CheckError {
    CheckError {
        kind: CheckErrorKind::MalformedInput,
    }
}
fn field(input: &Value, key: &str) -> Result<String, CheckError> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(malformed)
}

fn folded_tool_name(name: &str) -> String {
    // Fold one Unicode scalar at a time; full mappings would expand U+0130.
    name.chars()
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect()
}

pub fn decode(
    consumer: Consumer,
    bytes: &[u8],
    default_cwd: &str,
) -> Result<CanonicalEvent, CheckError> {
    decode_protocol(consumer, bytes, default_cwd, Protocol::Tool)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Protocol {
    Tool,
    Native,
}

pub(crate) fn decode_protocol(
    consumer: Consumer,
    bytes: &[u8],
    default_cwd: &str,
    protocol: Protocol,
) -> Result<CanonicalEvent, CheckError> {
    if protocol == Protocol::Native && std::str::from_utf8(bytes).is_err() {
        return Err(CheckError {
            kind: CheckErrorKind::InvalidEncoding,
        });
    }
    let value: Value =
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))?;
    if !value.is_object() {
        return Err(malformed());
    }
    let normalized = value.get("tool_input").is_some();
    let raw_codex_shell = consumer == Consumer::Codex
        && !normalized
        && value.get("arguments").is_some()
        && value
            .get("tool_name")
            .or_else(|| value.get("name"))
            .and_then(Value::as_str)
            .is_some_and(|name| ["exec_command", "functions.exec_command"].contains(&name));
    if protocol == Protocol::Native
        && !raw_codex_shell
        && value.get("tool_input").is_none_or(Value::is_null)
        && value.get("cwd").and_then(Value::as_str).is_none()
    {
        return Err(malformed());
    }
    let (name, input) = if normalized || protocol == Protocol::Native && !raw_codex_shell {
        (
            if protocol == Protocol::Native {
                value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("Bash")
            } else {
                value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .ok_or_else(malformed)?
            },
            value.get("tool_input").cloned().unwrap_or(Value::Null),
        )
    } else {
        match consumer {
            Consumer::Claude => return Err(malformed()),
            Consumer::Codex => {
                let name = value
                    .get("tool_name")
                    .or_else(|| value.get("name"))
                    .and_then(Value::as_str)
                    .ok_or_else(malformed)?;
                let mut input = value.get("arguments").cloned().ok_or_else(malformed)?;
                if let Some(text) = input.as_str() {
                    input = serde_json::from_str(text)?;
                }
                (name, input)
            }
            Consumer::Pi => (
                value
                    .get("toolName")
                    .and_then(Value::as_str)
                    .ok_or_else(malformed)?,
                value.get("input").cloned().ok_or_else(malformed)?,
            ),
        }
    };
    let workdir = if consumer == Consumer::Codex
        && ["exec_command", "functions.exec_command"].contains(&name)
    {
        input.get("workdir")
    } else {
        None
    };
    let cwd = workdir
        .or_else(|| value.get("cwd"))
        .or_else(|| input.get("cwd"))
        .or_else(|| input.get("workdir"))
        .map(|v| v.as_str().ok_or_else(malformed))
        .transpose()?
        .unwrap_or(default_cwd)
        .to_owned();
    if cwd.is_empty() || !std::path::Path::new(&cwd).is_absolute() {
        return Err(if protocol == Protocol::Native {
            CheckError {
                kind: CheckErrorKind::RelativeCwd,
            }
        } else {
            malformed()
        });
    }
    let folded_name = folded_tool_name(name);
    // Native stdin uses file_path for every runtime; Pi's tool-facing input
    // is a separate protocol with its own path field.
    let pi_path = if protocol == Protocol::Native {
        "file_path"
    } else {
        "path"
    };
    let operation = match (consumer, folded_name.as_str()) {
        (Consumer::Claude | Consumer::Codex | Consumer::Pi, "bash") => {
            Operation::Shell(field(&input, "command")?)
        }
        (Consumer::Codex, "exec_command" | "functions.exec_command")
            if ["exec_command", "functions.exec_command"].contains(&name) =>
        {
            Operation::Shell(field(&input, "cmd")?)
        }
        (Consumer::Claude | Consumer::Codex, "read") => {
            Operation::Read(field(&input, "file_path")?)
        }
        (Consumer::Pi, "read") => Operation::Read(field(&input, pi_path)?),
        (Consumer::Claude | Consumer::Codex, "write" | "edit") => {
            Operation::Write(field(&input, "file_path")?)
        }
        (Consumer::Pi, "write" | "edit") => Operation::Write(field(&input, pi_path)?),
        (Consumer::Claude | Consumer::Codex | Consumer::Pi, "grep") => Operation::Search {
            root: input
                .get("path")
                .map(|_| field(&input, "path"))
                .transpose()?
                .unwrap_or_default(),
            glob: input
                .get("glob")
                .map(|_| field(&input, "glob"))
                .transpose()?
                .unwrap_or_default(),
        },
        _ => Operation::Outside(name.to_owned()),
    };
    Ok(CanonicalEvent {
        operation,
        cwd,
        tool: name.to_owned(),
        input,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wire {
    pub exit: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn recovery_value(recovery: &Recovery) -> Value {
    let next = match &recovery.next_step {
        RecoveryStep::RecheckOperation { description } => {
            json!({"kind":"recheck_operation","description":description})
        }
        RecoveryStep::OwnerAction { description } => {
            json!({"kind":"owner_action","description":description})
        }
        RecoveryStep::StructuredOperation {
            tool,
            input,
            cwd,
            description,
        } => {
            json!({"kind":"structured_operation","tool":tool,"input":input,"cwd":cwd,"description":description})
        }
    };
    json!({"next_step":next,"preserved_scope":recovery.preserved_scope,"excluded_scope":recovery.excluded_scope,"automatic_application_supported":recovery.automatic_application_supported})
}

pub fn render(consumer: Consumer, result: &Result<Evaluation, CheckError>) -> Wire {
    let mut wire = Wire {
        exit: 0,
        stdout: String::new(),
        stderr: String::new(),
    };
    let reason = match result {
        Err(error) => {
            wire.exit = 2;
            wire.stderr = denial_text(consumer, error.consumer_message());
            return wire;
        }
        Ok(evaluation) => match &evaluation.outcome {
            Outcome::NoObjection => return wire,
            Outcome::SoftAdvice(advice) => {
                if consumer == Consumer::Claude {
                    let context = advice
                        .iter()
                        .map(|a| a.message())
                        .collect::<Vec<_>>()
                        .join("\n");
                    // Keep the established field order for byte-stable hook output.
                    let context = json!(context);
                    wire.stdout = format!(
                        "{{\"hookSpecificOutput\":{{\"hookEventName\":\"PreToolUse\",\"additionalContext\":{context}}}}}\n"
                    );
                }
                return wire;
            }
            Outcome::ProtectedDenial { reason, .. } => reason.rule.message().to_owned(),
            Outcome::CoverageInsufficient {
                disposition: Disposition::ContinueLimitedPreflight,
                ..
            } => return wire,
            Outcome::CoverageInsufficient { cause, .. } => {
                crate::policy::refusal_message(cause).to_owned()
            }
        },
    };
    wire.exit = 2;
    wire.stderr = denial_text(consumer, &reason);
    wire
}

fn denial_text(consumer: Consumer, reason: &str) -> String {
    if consumer == Consumer::Claude {
        format!(
            "DENIED: {reason} Do NOT bypass this restriction or retry the same blocked command.\n"
        )
    } else {
        format!("{reason}\n")
    }
}

/// Consumers authorize a call from the hook wire, never from an internal verdict label.
pub fn permits_call(consumer: Consumer, wire: &Wire) -> bool {
    match consumer {
        Consumer::Claude | Consumer::Codex => wire.exit != 2,
        Consumer::Pi => wire.exit == 0,
    }
}
