use crate::{CheckError, CheckErrorKind, Disposition, Evaluation, Outcome, Recovery, RecoveryStep};
use serde_json::{Value, json};

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

/// Caller-owned public continuation data, separate from submitted operations and verdicts.
pub enum PublicTask {
    Read { path: String },
    Search { pattern: String, glob: String },
    Write { path: String },
    LiteralFile { path: String, content: String },
    Redirect { path: String, content: String },
    Emit { literal: String },
    Script { source: String },
    List { path: String },
    HomeSetting,
}

fn proposed(tool: &str, input: Value, cwd: &str) -> RecoveryStep {
    RecoveryStep::StructuredOperation {tool:tool.to_owned(),input,cwd:cwd.to_owned(),description:"Use only this explicit public scope and cwd, retain the original objective, and recheck through the same consumer before applying it.".into()}
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

pub fn public_recovery(consumer: Consumer, project: &str, task: &PublicTask) -> RecoveryStep {
    let shell = if consumer == Consumer::Pi {
        "bash"
    } else {
        "Bash"
    };
    let proposed = |tool, input| proposed(tool, input, project);
    match task {
        PublicTask::Read {path}=>match consumer {
            Consumer::Claude=>proposed("Read",json!({"file_path":path})),Consumer::Pi=>proposed("read",json!({"path":path})),Consumer::Codex=>proposed(shell,json!({"command":format!("cat {}",quote(path))})),
        },
        PublicTask::Search {pattern,glob}=>match consumer {
            Consumer::Claude=>proposed("Grep",json!({"path":project,"pattern":pattern,"glob":glob})),Consumer::Pi=>proposed("grep",json!({"path":project,"pattern":pattern,"glob":glob})),Consumer::Codex=>proposed(shell,json!({"command":format!("rg -n {} -- {} {}",if glob.is_empty() {String::new()} else {format!("-g {}",quote(glob))},quote(pattern),quote(project))})),
        },
        PublicTask::LiteralFile {path,content}=>match consumer {
            Consumer::Claude=>proposed("Write",json!({"file_path":path,"content":content})),Consumer::Pi=>proposed("write",json!({"path":path,"content":content})),Consumer::Codex=>RecoveryStep::OwnerAction {description:"Provide a verified, enrolled structured Write/Edit operation for literal text, then recheck it; Codex has no covered structured writer in this P1 slice.".into()},
        },
        PublicTask::Redirect {path,content}=>proposed(shell,json!({"command":format!("printf '%s' {} > {}",quote(content),quote(path))})),
        PublicTask::Emit {literal}=>proposed(shell,json!({"command":if literal=="ok" {"printf ok".to_owned()} else {format!("printf '%s\\n' {}",quote(literal))}})),
        PublicTask::Script {source}=>proposed(shell,json!({"command":source})),
        PublicTask::List {path}=>proposed(shell,json!({"command":format!("ls {}",quote(path))})),
        PublicTask::HomeSetting=>proposed(shell,json!({"command":"printf '%s\\n' \"$HOME\""})),
        PublicTask::Write {..}=>RecoveryStep::OwnerAction {description:"Supply the public structured write with its original payload and recheck before applying it.".into()},
    }
}

pub fn write_recovery(
    consumer: Consumer,
    project: &str,
    task: &PublicTask,
    event: &CanonicalEvent,
) -> RecoveryStep {
    if let PublicTask::Write { path } = task {
        let mut input = event.input.clone();
        input[if consumer == Consumer::Pi {
            "path"
        } else {
            "file_path"
        }] = json!(path);
        proposed(&event.tool, input, project)
    } else {
        RecoveryStep::OwnerAction {description:"Provide the original payload at an explicit public write target, then recheck the operation.".into()}
    }
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

pub fn decode(
    consumer: Consumer,
    bytes: &[u8],
    default_cwd: &str,
) -> Result<CanonicalEvent, CheckError> {
    let value: Value =
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))?;
    if !value.is_object() {
        return Err(malformed());
    }
    let normalized = value.get("tool_input").is_some();
    let (name, input) = if normalized {
        (
            value
                .get("tool_name")
                .and_then(Value::as_str)
                .ok_or_else(malformed)?,
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
    let cwd = value
        .get("cwd")
        .or_else(|| input.get("cwd"))
        .or_else(|| input.get("workdir"))
        .map(|v| v.as_str().ok_or_else(malformed))
        .transpose()?
        .unwrap_or(default_cwd)
        .to_owned();
    let operation = match (consumer, name) {
        (Consumer::Claude, "Bash") | (Consumer::Codex, "Bash") | (Consumer::Pi, "bash") => {
            Operation::Shell(field(&input, "command")?)
        }
        (Consumer::Codex, "exec_command" | "functions.exec_command") => {
            Operation::Shell(field(&input, "cmd")?)
        }
        (Consumer::Claude, "Read") | (Consumer::Codex, "Read") => {
            Operation::Read(field(&input, "file_path")?)
        }
        (Consumer::Pi, "read") => Operation::Read(field(&input, "path")?),
        (Consumer::Claude, "Write" | "Edit") | (Consumer::Codex, "Write" | "Edit") => {
            Operation::Write(field(&input, "file_path")?)
        }
        (Consumer::Pi, "write" | "edit") => Operation::Write(field(&input, "path")?),
        (Consumer::Claude, "Grep") | (Consumer::Codex, "Grep") | (Consumer::Pi, "grep") => {
            Operation::Search {
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
            }
        }
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
    json!({"next_step":next,"objective":recovery.objective,"preserved_scope":recovery.preserved_scope,"excluded_scope":recovery.excluded_scope,"automatic_application_supported":recovery.automatic_application_supported})
}

pub fn render(consumer: Consumer, result: &Result<Evaluation, CheckError>) -> Wire {
    let mut wire = Wire {
        exit: 0,
        stdout: String::new(),
        stderr: String::new(),
    };
    let (reason, recovery) = match result {
        Err(error) => {
            wire.exit = 3;
            wire.stderr = format!(
                "Check incomplete: {error}. {}, then recheck without raw input logging.\n",
                if error.kind == CheckErrorKind::ProbeFault {
                    "Have the owner repair access to the non-sensitive probe prefix"
                } else {
                    "Reduce input/nesting to the frozen supported bound or repair the checker"
                }
            );
            return wire;
        }
        Ok(evaluation) => match &evaluation.outcome {
            Outcome::NoObjection => return wire,
            Outcome::SoftAdvice(advice) => {
                if consumer == Consumer::Claude {
                    wire.stdout = format!(
                        "{}\n",
                        json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":advice.iter().map(|a|a.message.as_str()).collect::<Vec<_>>().join("\n")}})
                    );
                }
                return wire;
            }
            Outcome::ProtectedDenial { reason, recovery } => {
                (reason.effect.clone(), Some(recovery))
            }
            Outcome::CoverageInsufficient {
                disposition: Disposition::ContinueLimitedPreflight,
                ..
            } => return wire,
            Outcome::CoverageInsufficient {
                cause, recovery, ..
            } => (
                format!("unsupported preflight: {cause:?}"),
                recovery.as_ref(),
            ),
        },
    };
    wire.exit = 2;
    let detail = recovery.map(recovery_value).unwrap_or(Value::Null);
    wire.stderr = format!(
        "{}{}; recovery: {}\n",
        if consumer == Consumer::Claude {
            "DENIED: "
        } else {
            ""
        },
        reason,
        detail
    );
    wire
}
