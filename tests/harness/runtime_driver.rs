use super::{
    Result,
    model::{ModelServer, ToolResult},
    process::*,
    registration::*,
    runtime::*,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::os::unix::fs::OpenOptionsExt;
use std::{
    env,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    time::Duration,
};

pub struct RuntimeOptions {
    pub source: PathBuf,
    pub entry: PathBuf,
    pub output: PathBuf,
    pub helper: PathBuf,
    pub runtimes: Vec<String>,
    pub ablate: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeClient {
    pub runtime: String,
    pub executable: Binding,
    pub version: ProcessResult,
    #[serde(
        rename = "identityError",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub identity_error: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeRow {
    pub runtime: String,
    pub run: usize,
    pub id: String,
    pub tool: String,
    pub input: Value,
    pub expected: String,
    #[serde(flatten)]
    pub verdict: Verdict,
    #[serde(flatten)]
    pub process: ProcessResult,
    pub hooks: Vec<HookObservation>,
    #[serde(rename = "runtimeResult")]
    pub result: Option<ToolResult>,
    #[serde(
        rename = "modelError",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub model_error: String,
    #[serde(
        rename = "traceError",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub trace_error: String,
    pub requests: usize,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeSummary {
    pub runtime: String,
    #[serde(rename = "plannedCalls")]
    pub planned: usize,
    #[serde(rename = "attemptedCalls")]
    pub attempted: usize,
    pub verified: usize,
    #[serde(rename = "matchedCalls")]
    pub matched: usize,
    #[serde(rename = "falseAllow")]
    pub false_allow: Option<usize>,
    #[serde(rename = "falseDeny")]
    pub false_deny: Option<usize>,
    #[serde(rename = "runtimeGuardDisagreements")]
    pub disagreements: usize,
    #[serde(rename = "guardP50Ms")]
    pub hook_p50: Option<f64>,
    #[serde(rename = "guardP95Ms")]
    pub hook_p95: Option<f64>,
    pub complete: bool,
}
pub fn runtime_summaries(
    rows: &[RuntimeRow],
    runtimes: &[String],
    ablate: bool,
) -> Vec<RuntimeSummary> {
    runtimes
        .iter()
        .map(|runtime| {
            let mut summary = RuntimeSummary {
                runtime: runtime.clone(),
                planned: CORPUS.iter().filter(|case| case.applies(runtime)).count() * 3,
                attempted: 0,
                verified: 0,
                matched: 0,
                false_allow: None,
                false_deny: None,
                disagreements: 0,
                hook_p50: None,
                hook_p95: None,
                complete: false,
            };
            let mut times = Vec::new();
            let mut valid = true;
            for row in rows.iter().filter(|row| &row.runtime == runtime) {
                summary.attempted += 1;
                if row.verdict.runtime != "unverified" {
                    summary.verified += 1;
                    if row.expected == "allow" {
                        *summary.false_deny.get_or_insert(0) +=
                            usize::from(row.verdict.runtime == "deny");
                    }
                    if row.expected == "deny" {
                        *summary.false_allow.get_or_insert(0) +=
                            usize::from(row.verdict.runtime == "allow");
                    }
                }
                if row.verdict.runtime == if ablate { "allow" } else { &row.expected } {
                    summary.matched += 1;
                }
                summary.disagreements += usize::from(row.verdict.conflict);
                valid &= row.process.status == 0
                    && !row.process.timed_out
                    && row.process.spawn_error.is_empty()
                    && row.process.wait_error.is_empty()
                    && row.model_error.is_empty()
                    && row.trace_error.is_empty()
                    && row.hooks.len() == 1
                    && row.verdict.guard != "unverified";
                for hook in &row.hooks {
                    times.push(hook.process.milliseconds);
                    valid &= !hook.process.timed_out
                        && hook.process.spawn_error.is_empty()
                        && hook.process.wait_error.is_empty()
                        && hook.observation_error.is_empty()
                        && hook.survivors.is_empty();
                }
            }
            times.sort_by(f64::total_cmp);
            if !times.is_empty() {
                summary.hook_p50 = Some(times[(times.len() - 1) / 2]);
                summary.hook_p95 = Some(times[(times.len() * 95).div_ceil(100) - 1]);
            }
            summary.complete = valid
                && summary.attempted == summary.planned
                && summary.verified == summary.planned
                && summary.matched == summary.planned
                && summary.disagreements == 0;
            summary
        })
        .collect()
}
struct SyntheticHome(PathBuf);
impl Drop for SyntheticHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub fn run_runtime(options: &RuntimeOptions) -> Result<()> {
    if !options.entry.is_absolute() || !options.helper.is_absolute() {
        return Err("entry and helper must be absolute paths".into());
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "The runtime driver binds the selected assembled entry's resolved identity."
    )]
    let entry = fs::canonicalize(&options.entry)?;
    if entry.file_name() != Some(std::ffi::OsStr::new("agent-guard"))
        || entry.parent().and_then(Path::file_name) != Some(std::ffi::OsStr::new("bin"))
    {
        return Err("select the assembled bin/agent-guard executable".into());
    }
    if options.runtimes.is_empty() {
        return Err("select claude, pi, or codex".into());
    }
    for runtime in &options.runtimes {
        if !["claude", "pi", "codex"].contains(&runtime.as_str()) {
            return Err(format!("unknown runtime {runtime:?}").into());
        }
    }
    let output = new_output(&options.output)?;
    let source = source_bindings(&options.source)?;
    let guide = fs::read_to_string(options.source.join("docs/setup.md"))?;
    let synthetic = SyntheticHome(synthetic_home(&output)?);
    let home = &synthetic.0;
    let manifest = copy_installation(&entry, home)?;
    let helper_binding = hash_file(&options.helper, "runtime-harness")?;
    let mut server = ModelServer::start()?;
    let url = &server.url;
    let trace = home.join("trace.jsonl");
    let environment = runtime_environment(options, home, url, &trace);
    let mut records = Vec::new();
    let mut clients = Vec::new();
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(output.join("records.jsonl"))?;
    for runtime in &options.runtimes {
        let (identity, client) =
            runtime_client(runtime, home, &options.helper, url, &guide, &environment)?;
        clients.push(identity);
        'runtime_calls: for repeat in 0..3 {
            for fixture in CORPUS.iter().filter(|case| case.applies(runtime)) {
                let row = runtime_call(
                    runtime,
                    repeat,
                    fixture,
                    &client,
                    home,
                    &environment,
                    &server,
                )?;
                json_line(&mut file, &row)?;
                println!(
                    "{runtime} {repeat} {} {} status={} hooks={}",
                    fixture.id,
                    row.verdict.runtime,
                    row.process.status,
                    row.hooks.len()
                );
                let startup_failed = (row.process.status != 0
                    || row.process.timed_out
                    || !row.process.spawn_error.is_empty())
                    && row.requests == 0
                    && row.hooks.is_empty();
                records.push(row);
                if startup_failed {
                    break 'runtime_calls;
                }
            }
        }
    }
    let mut summaries = runtime_summaries(&records, &options.runtimes, options.ablate);
    for (summary, client) in summaries.iter_mut().zip(&clients) {
        if !client.identity_error.is_empty()
            || client.version.status != 0
            || client.version.timed_out
            || !client.version.wait_error.is_empty()
        {
            summary.complete = false;
        }
    }
    write_json(
        &output.join("report.json"),
        &json!({"manifest":manifest,"source":source,"helper":helper_binding,"clients":clients,"ablated":options.ablate,"summary":summaries,"records":records,"survivalCoverage":"Entry PID only. Runner/checker/watchdog/descendant cleanup requires the separately instrumented lifecycle driver."}),
    )?;
    server.finish()?;
    if summaries.iter().any(|s| !s.complete) {
        return Err("runtime evaluation is incomplete or mismatched; see report.json".into());
    }
    Ok(())
}
fn runtime_environment(
    options: &RuntimeOptions,
    home: &Path,
    url: &str,
    trace: &Path,
) -> Vec<(String, String)> {
    vec![
        ("HOME", home.to_string_lossy().into()),
        ("PATH", env::var("PATH").unwrap_or_default()),
        ("TMPDIR", home.to_string_lossy().into()),
        ("CODEX_HOME", home.join(".codex").to_string_lossy().into()),
        (
            "CLAUDE_CONFIG_DIR",
            home.join(".claude").to_string_lossy().into(),
        ),
        ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1".into()),
        ("ANTHROPIC_BASE_URL", url.into()),
        ("ANTHROPIC_API_KEY", "synthetic-not-a-credential".into()),
        ("AGENT_GUARD_TEST_MODEL_URL", url.into()),
        (
            "AGENT_GUARD_TEST_ENTRY",
            home.join("installation/bin/agent-guard")
                .to_string_lossy()
                .into(),
        ),
        (
            "AGENT_GUARD_TEST_HOOK_TRACE",
            trace.to_string_lossy().into(),
        ),
        (
            "AGENT_GUARD_TEST_ABLATE",
            if options.ablate { "1" } else { "0" }.into(),
        ),
        (
            "AGENT_GUARD_TEST_HELPER",
            options.helper.to_string_lossy().into(),
        ),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value))
    .collect()
}

fn runtime_client(
    runtime: &str,
    home: &Path,
    helper: &Path,
    url: &str,
    guide: &str,
    environment: &[(String, String)],
) -> Result<(RuntimeClient, PathBuf)> {
    let mut identity = RuntimeClient {
        runtime: runtime.into(),
        executable: Binding {
            path: String::new(),
            sha256: String::new(),
            mode: 0,
        },
        version: ProcessResult::default(),
        identity_error: String::new(),
    };
    let client = match look_path(runtime) {
        Ok(path) => {
            #[expect(
                clippy::disallowed_methods,
                reason = "The runtime client owner resolves the selected executable before recording its binding."
            )]
            let path = fs::canonicalize(path)?;
            identity.executable = hash_file(&path, &path.to_string_lossy())?;
            path
        }
        Err(e) => {
            identity.identity_error = e.to_string();
            PathBuf::from(runtime)
        }
    };
    runtime_config(home, runtime, &client, helper, url, guide)?;
    identity.version = run(
        &[client.to_string_lossy().into(), "--version".into()],
        &[],
        &home.join("workspace"),
        environment,
        Duration::from_secs(5),
    )?;
    Ok((identity, client))
}

fn runtime_call(
    runtime: &str,
    repeat: usize,
    fixture: &RuntimeCase,
    client: &Path,
    home: &Path,
    environment: &[(String, String)],
    server: &ModelServer,
) -> Result<RuntimeRow> {
    let trace = home.join("trace.jsonl");
    let (tool, input) = fixture.tool_call(runtime, home);
    server
        .model
        .lock()
        .map_err(|_| "model lock poisoned")?
        .begin(runtime, tool, input.clone());
    write_file(&trace, &[], 0o600)?;
    fixture.prepare(home)?;
    let mut environment = environment.to_vec();
    if fixture.silent_guard {
        environment.push(("AGENT_GUARD_TEST_ENTRY".into(), "/usr/bin/false".into()));
    }
    let process = run(
        &runtime_args(runtime, client, home),
        &[],
        &home.join("workspace"),
        &environment,
        Duration::from_secs(20),
    )?;
    let (hooks, trace_error) = match read_hooks(&trace) {
        Ok(hooks) => (hooks, String::new()),
        Err(e) => (Vec::new(), e.to_string()),
    };
    let state = server.model.lock().map_err(|_| "model lock poisoned")?;
    let row = RuntimeRow {
        runtime: runtime.into(),
        run: repeat,
        id: fixture.id.into(),
        tool: tool.into(),
        input,
        expected: fixture.expected.into(),
        verdict: runtime_verdict(runtime, &hooks, state.result.as_ref(), fixture, home),
        process,
        hooks,
        result: state.result.clone(),
        model_error: state.error.clone(),
        trace_error,
        requests: state.requests,
    };
    drop(state);
    Ok(row)
}

fn copy_installation(entry: &Path, home: &Path) -> Result<Vec<Binding>> {
    let mut manifest = Vec::new();
    for name in ["agent-guard", "agent-guard-native"] {
        let from = if name == "agent-guard" {
            entry.to_path_buf()
        } else {
            entry.parent().ok_or("missing entry parent")?.join(name)
        };
        let to = home.join("installation/bin").join(name);
        copy_file(&from, &to)?;
        manifest.push(hash_file(&to, &format!("bin/{name}"))?);
    }
    Ok(manifest)
}
