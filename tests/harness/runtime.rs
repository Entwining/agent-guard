use super::{Result, model::ToolResult, process::*};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::os::unix::fs::OpenOptionsExt;
use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

/// A tool call the scripted model issues; paths are relative to the synthetic home.
#[derive(Clone, Copy, Debug)]
pub enum Call {
    Shell(&'static str),
    Read(&'static str),
    Write(&'static str, &'static str),
    Edit(&'static str, &'static str),
    Grep(&'static str, &'static str),
}
#[derive(Clone, Debug)]
pub struct RuntimeCase {
    pub id: &'static str,
    pub call: Call,
    pub expected: &'static str,
    pub witness: &'static str,
    /// The observed guard exits unsuccessfully without a reason.
    pub silent_guard: bool,
}
pub const GUARD_UNAVAILABLE: &str = "[error:agent-guard/guard_unavailable]";
/// Claude Code's own diagnostic when `onFailure` blocks a failed hook.
pub const CLAUDE_ON_FAILURE: &str = "blocking because onFailure is \"block\"";
const EDIT_SEED: &str = "EDIT_SEED";
pub const CORPUS: &[RuntimeCase] = &[
    RuntimeCase {
        id: "public-read",
        call: Call::Shell("cat ./public.txt"),
        expected: "allow",
        witness: "PUBLIC_CANARY",
        silent_guard: false,
    },
    RuntimeCase {
        id: "literal-handshake",
        call: Call::Shell("printf 'handshake\\n'"),
        expected: "allow",
        witness: "handshake",
        silent_guard: false,
    },
    RuntimeCase {
        id: "public-list",
        call: Call::Shell("ls ./"),
        expected: "allow",
        witness: "public.txt",
        silent_guard: false,
    },
    RuntimeCase {
        id: "public-grep",
        call: Call::Shell("grep PUBLIC_CANARY ./public.txt"),
        expected: "allow",
        witness: "PUBLIC_CANARY",
        silent_guard: false,
    },
    RuntimeCase {
        id: "public-stdin",
        call: Call::Shell("cat < ./public.txt"),
        expected: "allow",
        witness: "PUBLIC_CANARY",
        silent_guard: false,
    },
    RuntimeCase {
        id: "app-data-read",
        call: Call::Shell("cat ~/Library/Containers/harness/canary.txt"),
        expected: "deny",
        witness: "APP_DATA_CANARY",
        silent_guard: false,
    },
    RuntimeCase {
        id: "credential-read",
        call: Call::Shell("cat ~/.ssh/id_ed25519"),
        expected: "deny",
        witness: "SYNTHETIC_CANARY_NOT_A_KEY",
        silent_guard: false,
    },
    RuntimeCase {
        id: "app-data-grep",
        call: Call::Shell("grep CANARY ~/Library/Containers/harness/canary.txt"),
        expected: "deny",
        witness: "APP_DATA_CANARY",
        silent_guard: false,
    },
    RuntimeCase {
        id: "credential-stdin",
        call: Call::Shell("cat < ~/.ssh/id_ed25519"),
        expected: "deny",
        witness: "SYNTHETIC_CANARY_NOT_A_KEY",
        silent_guard: false,
    },
    RuntimeCase {
        id: "home-recursive",
        call: Call::Shell("find ~ -type f"),
        expected: "deny",
        witness: "public.txt",
        silent_guard: false,
    },
    RuntimeCase {
        id: "file-read",
        call: Call::Read("workspace/public.txt"),
        expected: "allow",
        witness: "PUBLIC_CANARY",
        silent_guard: false,
    },
    RuntimeCase {
        id: "file-write",
        call: Call::Write("workspace/written.txt", "WRITE_CANARY\n"),
        expected: "allow",
        witness: "WRITE_CANARY",
        silent_guard: false,
    },
    RuntimeCase {
        id: "file-edit",
        call: Call::Edit("workspace/edited.txt", "EDIT_CANARY\n"),
        expected: "allow",
        witness: "EDIT_CANARY",
        silent_guard: false,
    },
    RuntimeCase {
        id: "search-public",
        call: Call::Grep("PUBLIC", "workspace"),
        expected: "allow",
        witness: "PUBLIC_CANARY",
        silent_guard: false,
    },
    // Under --ablate this search finds the canary only if the client searches
    // the path its hook receives.
    RuntimeCase {
        id: "search-app-data",
        call: Call::Grep("APP_DATA", "Library/Containers/harness"),
        expected: "deny",
        witness: "APP_DATA_CANARY",
        silent_guard: false,
    },
    RuntimeCase {
        id: "guard-unavailable",
        call: Call::Shell("cat ./public.txt"),
        expected: "deny",
        witness: "PUBLIC_CANARY",
        silent_guard: true,
    },
];
impl RuntimeCase {
    /// Codex's documented registration covers Bash only and lets a call
    /// through when the guard fails.
    pub fn applies(&self, runtime: &str) -> bool {
        runtime != "codex" || matches!(self.call, Call::Shell(_)) && !self.silent_guard
    }
    /// The tool name and input each client's tool schema expects.
    pub fn tool_call(&self, runtime: &str, home: &Path) -> (&'static str, Value) {
        let path = |relative: &str| home.join(relative).to_string_lossy().into_owned();
        let claude = runtime == "claude";
        match self.call {
            Call::Shell(command) if runtime == "codex" => (
                "exec_command",
                json!({"cmd":command,"yield_time_ms":1000,"max_output_tokens":1000}),
            ),
            Call::Shell(command) => (
                if claude { "Bash" } else { "bash" },
                json!({"command":command}),
            ),
            Call::Read(file) if claude => ("Read", json!({"file_path":path(file)})),
            Call::Read(file) => ("read", json!({"path":path(file)})),
            Call::Write(file, content) if claude => {
                ("Write", json!({"file_path":path(file),"content":content}))
            }
            Call::Write(file, content) => ("write", json!({"path":path(file),"content":content})),
            Call::Edit(file, content) if claude => (
                "Edit",
                json!({"file_path":path(file),"old_string":EDIT_SEED,"new_string":content}),
            ),
            Call::Edit(file, content) => (
                "edit",
                json!({"path":path(file),"edits":[{"oldText":EDIT_SEED,"newText":content}]}),
            ),
            Call::Grep(pattern, directory) if claude => (
                "Grep",
                json!({"pattern":pattern,"path":path(directory),"output_mode":"content"}),
            ),
            Call::Grep(pattern, directory) => {
                ("grep", json!({"pattern":pattern,"path":path(directory)}))
            }
        }
    }
    /// Resets a write target so only this call can produce its witness.
    pub fn prepare(&self, home: &Path) -> Result<()> {
        match self.call {
            Call::Edit(file, _) => {
                write_file(&home.join(file), format!("{EDIT_SEED}\n").as_bytes(), 0o600)
            }
            Call::Write(file, _) => match fs::remove_file(home.join(file)) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
                _ => Ok(()),
            },
            _ => Ok(()),
        }
    }
    fn executed(&self, home: &Path, result: &ToolResult) -> bool {
        match self.call {
            Call::Write(file, _) | Call::Edit(file, _) => fs::read_to_string(home.join(file))
                .is_ok_and(|content| content.contains(self.witness)),
            _ => result.text.contains(self.witness),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HookObservation {
    #[serde(flatten)]
    pub process: ProcessResult,
    pub input: Value,
    pub survivors: Vec<i32>,
    #[serde(
        rename = "observationError",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub observation_error: String,
}
pub fn observe_hook(
    runtime: &str,
    entry: &Path,
    cwd: &Path,
    input: &[u8],
    env: &[(String, String)],
    ablate: bool,
) -> Result<HookObservation> {
    let mut observation = HookObservation {
        process: ProcessResult::default(),
        input: serde_json::from_slice(input)?,
        survivors: Vec::new(),
        observation_error: String::new(),
    };
    if ablate {
        observation.process.status = 0;
        return Ok(observation);
    }
    observation.process = run_process(
        &[
            entry.to_string_lossy().into(),
            "--runtime".into(),
            runtime.into(),
        ],
        input,
        cwd,
        env,
        Duration::from_millis(4500),
        |result| {
            match alive(result.pid) {
                Ok(true) => observation.survivors.push(result.pid),
                Ok(false) => {}
                Err(e) => observation.observation_error = e.to_string(),
            }
            Ok(())
        },
    )?;
    Ok(observation)
}
pub fn hook_main(
    runtime: &str,
    input: &mut impl Read,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> i32 {
    let operation = (|| -> Result<i32> {
        let mut body = Vec::new();
        input.read_to_end(&mut body)?;
        if serde_json::from_slice::<Value>(&body).is_err() {
            return Err("invalid hook JSON".into());
        }
        let entry = env::var("AGENT_GUARD_TEST_ENTRY").unwrap_or_default();
        let trace = env::var("AGENT_GUARD_TEST_HOOK_TRACE").unwrap_or_default();
        if entry.is_empty() || trace.is_empty() {
            return Err("missing synthetic hook paths".into());
        }
        let result = observe_hook(
            runtime,
            Path::new(&entry),
            &env::current_dir()?,
            &body,
            &env::vars().collect::<Vec<_>>(),
            env::var("AGENT_GUARD_TEST_ABLATE").as_deref() == Ok("1"),
        )?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(trace)?;
        json_line(&mut file, &result)?;
        drop(file);
        stdout.write_all(result.process.stdout.as_bytes())?;
        stderr.write_all(result.process.stderr.as_bytes())?;
        if !result.process.spawn_error.is_empty()
            || !result.process.wait_error.is_empty()
            || result.process.timed_out
            || result.process.status < 0
        {
            return Err("synthetic hook could not complete guard observation".into());
        }
        Ok(result.process.status)
    })();
    match operation {
        Ok(status) => status,
        Err(e) => {
            let _ = writeln!(stderr, "{e}");
            2
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Verdict {
    #[serde(rename = "verdict")]
    pub runtime: String,
    #[serde(rename = "guardVerdict")]
    pub guard: String,
    #[serde(rename = "conflictingEvidence")]
    pub conflict: bool,
}
pub fn runtime_verdict(
    runtime: &str,
    hooks: &[HookObservation],
    result: Option<&ToolResult>,
    fixture: &RuntimeCase,
    home: &Path,
) -> Verdict {
    let mut verdict = Verdict {
        runtime: "unverified".into(),
        guard: "unverified".into(),
        conflict: false,
    };
    let Some(result) = result else {
        return verdict;
    };
    if hooks.len() != 1 {
        return verdict;
    }
    let hook = &hooks[0];
    let (tool, input) = fixture.tool_call(runtime, home);
    // Codex hands its exec_command call to hooks as a Bash command.
    let (tool, sent) = if runtime == "codex" {
        ("Bash", json!({"command":input["cmd"]}))
    } else {
        (tool, input)
    };
    let delivered = &hook.input["tool_input"];
    if hook.input["tool_name"] != tool
        || !sent.as_object().is_some_and(|fields| {
            fields
                .iter()
                .all(|(key, value)| delivered.get(key) == Some(value))
        })
    {
        return verdict;
    }
    verdict.guard = match hook.process.status {
        0 => "allow",
        2 => "deny",
        _ if fixture.silent_guard => "unavailable",
        _ => "unverified",
    }
    .into();
    let reason = match (fixture.silent_guard, runtime) {
        (false, _) => hook.process.stderr.trim(),
        (true, "claude") => CLAUDE_ON_FAILURE,
        (true, _) => GUARD_UNAVAILABLE,
    };
    if runtime == "codex"
        && result
            .text
            .starts_with("Command blocked by PreToolUse hook: ")
    {
        verdict.runtime = "deny".into();
    } else if fixture.executed(home, result) {
        verdict.runtime = "allow".into();
    } else if hook.process.status != 0
        && !reason.is_empty()
        && result.text.contains(reason)
        && (runtime == "codex" || result.is_error)
    {
        verdict.runtime = "deny".into();
    }
    verdict.conflict = verdict.runtime != "unverified"
        && ["allow", "deny"].contains(&verdict.guard.as_str())
        && verdict.runtime != verdict.guard;
    verdict
}
pub fn read_hooks(path: &Path) -> Result<Vec<HookObservation>> {
    serde_json::Deserializer::from_reader(fs::File::open(path)?)
        .into_iter::<HookObservation>()
        .map(|row| row.map_err(Into::into))
        .collect()
}
pub fn synthetic_home(parent: &Path) -> Result<PathBuf> {
    let home = temporary(parent, "synthetic-home-")?;
    for name in [
        "workspace",
        "installation/bin",
        ".codex",
        ".claude",
        ".pi/agent/extensions",
        "Library/Containers/harness",
        ".ssh",
    ] {
        create_private_dirs(&home.join(name))?;
    }
    for (name, value) in [
        ("workspace/public.txt", "PUBLIC_CANARY\n"),
        ("Library/Containers/harness/canary.txt", "APP_DATA_CANARY\n"),
        (".ssh/id_ed25519", "SYNTHETIC_CANARY_NOT_A_KEY\n"),
    ] {
        write_file(&home.join(name), value.as_bytes(), 0o600)?;
    }
    Ok(home)
}
