use super::Result;
use rustix::{
    fs::{OFlags, fcntl_getfl, fcntl_setfl},
    process::{Pid, Signal, kill_process, kill_process_group, test_kill_process},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessResult {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
    pub pid: i32,
    #[serde(rename = "timedOut")]
    pub timed_out: bool,
    #[serde(
        rename = "spawnError",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub spawn_error: String,
    #[serde(
        rename = "waitError",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub wait_error: String,
    #[serde(rename = "ms")]
    pub milliseconds: f64,
}
impl Default for ProcessResult {
    fn default() -> Self {
        Self {
            status: -1,
            stdout: String::new(),
            stderr: String::new(),
            pid: 0,
            timed_out: false,
            spawn_error: String::new(),
            wait_error: String::new(),
            milliseconds: 0.0,
        }
    }
}

pub fn kill_group(pid: i32) -> Result<()> {
    let pid = Pid::from_raw(pid).ok_or("invalid process group")?;
    match kill_process_group(pid, Signal::KILL) {
        Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
pub fn kill_pid(pid: i32) -> Result<()> {
    let pid = Pid::from_raw(pid).ok_or("invalid process identity")?;
    match kill_process(pid, Signal::KILL) {
        Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
pub fn alive(pid: i32) -> Result<bool> {
    let pid = Pid::from_raw(pid).ok_or("invalid process identity")?;
    match test_kill_process(pid) {
        Ok(()) => Ok(true),
        Err(rustix::io::Errno::SRCH) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

struct Group(i32);
impl Drop for Group {
    fn drop(&mut self) {
        if self.0 > 0 {
            let _ = kill_group(self.0);
        }
    }
}

#[derive(Debug)]
pub struct ProcessFailure {
    pub process: ProcessResult,
    pub cause: super::Error,
}
impl std::fmt::Display for ProcessFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.cause.fmt(f)
    }
}
impl std::error::Error for ProcessFailure {}

fn nonblocking(fd: &impl std::os::fd::AsFd) -> io::Result<()> {
    fcntl_setfl(fd, fcntl_getfl(fd)? | OFlags::NONBLOCK)?;
    Ok(())
}
fn drain(reader: &mut impl Read, bytes: &mut Vec<u8>) -> io::Result<bool> {
    let mut buffer = [0; 16384];
    // A continuous producer must leave time to feed stdin and poll the deadline.
    for _ in 0..32 {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(true),
            Ok(n) => bytes.extend_from_slice(&buffer[..n]),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(false)
}

// Poll all three pipes together: a full output pipe must not block input or the deadline.
// The observer sees descendants before the harness kills the invocation's group.
pub fn run_process<F>(
    argv: &[String],
    input: &[u8],
    cwd: &Path,
    env: &[(String, String)],
    deadline: Duration,
    observe: F,
) -> Result<ProcessResult>
where
    F: FnOnce(&ProcessResult) -> Result<()>,
{
    let started = Instant::now();
    let mut result = ProcessResult::default();
    let mut command = Command::new(argv.first().ok_or("empty process argv")?);
    command
        .args(&argv[1..])
        .current_dir(cwd)
        .env_clear()
        .envs(env.iter().cloned())
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            result.spawn_error = e.to_string();
            result.milliseconds = started.elapsed().as_secs_f64() * 1000.0;
            return Ok(result);
        }
    };
    result.pid = i32::try_from(child.id())?;
    let mut group = Group(result.pid);
    let operation = (|| -> Result<()> {
        let mut stdin = Some(child.stdin.take().ok_or("missing child stdin")?);
        let mut stdout = child.stdout.take().ok_or("missing child stdout")?;
        let mut stderr = child.stderr.take().ok_or("missing child stderr")?;
        nonblocking(stdin.as_ref().ok_or("missing child stdin")?)?;
        nonblocking(&stdout)?;
        nonblocking(&stderr)?;
        let mut offset = 0;
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut exited = None;
        loop {
            if let Some(pipe) = stdin.as_mut() {
                match pipe.write(&input[offset..]) {
                    Ok(n) => offset += n,
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(e) if e.kind() == io::ErrorKind::BrokenPipe => offset = input.len(),
                    Err(e) => return Err(e.into()),
                }
                if offset == input.len() {
                    stdin = None;
                }
            }
            let out_done = drain(&mut stdout, &mut out)?;
            let err_done = drain(&mut stderr, &mut err)?;
            if !result.timed_out && !deadline.is_zero() && started.elapsed() >= deadline {
                result.timed_out = true;
                kill_group(result.pid)?;
            }
            if exited.is_none()
                && let Some(status) = child.try_wait()?
            {
                result.status = status.code().unwrap_or(-1);
                exited = Some(Instant::now());
            }
            if let Some(at) = exited {
                if out_done && err_done {
                    break;
                }
                if at.elapsed() >= Duration::from_secs(1) {
                    result.wait_error = "exec: WaitDelay expired before I/O complete".into();
                    break;
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
        result.stdout = String::from_utf8_lossy(&out).into_owned();
        result.stderr = String::from_utf8_lossy(&err).into_owned();
        result.milliseconds = started.elapsed().as_secs_f64() * 1000.0;
        observe(&result)
    })();
    // On errors too, terminate first and wait for the direct child before returning.
    let cleanup = kill_group(result.pid);
    let wait = child.wait();
    group.0 = 0;
    let mut failures = Vec::new();
    if let Err(e) = operation {
        failures.push(e.to_string());
    }
    if let Err(e) = cleanup {
        failures.push(e.to_string());
    }
    if let Err(e) = wait {
        failures.push(e.to_string());
    }
    if !failures.is_empty() {
        return Err(Box::new(ProcessFailure {
            process: result,
            cause: failures.join("\n").into(),
        }));
    }
    Ok(result)
}

pub fn run(
    argv: &[String],
    input: &[u8],
    cwd: &Path,
    env: &[(String, String)],
    deadline: Duration,
) -> Result<ProcessResult> {
    run_process(argv, input, cwd, env, deadline, |_| Ok(()))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Binding {
    pub path: String,
    pub sha256: String,
    pub mode: u32,
}
#[expect(
    clippy::disallowed_methods,
    reason = "The evidence binding owner records the selected artifact's permission mode alongside its hash."
)]
pub fn hash_file(path: &Path, name: &str) -> Result<Binding> {
    Ok(Binding {
        path: name.into(),
        sha256: format!("{:x}", Sha256::digest(fs::read(path)?)),
        mode: fs::metadata(path)?.permissions().mode() & 0o777,
    })
}
pub fn write_file(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(mode)
        .open(path)?;
    file.write_all(bytes)?;
    Ok(())
}
pub fn create_private_dirs(path: &Path) -> Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    Ok(())
}
#[expect(
    clippy::disallowed_methods,
    reason = "The harness copy owner preserves the selected source file's permission mode."
)]
pub fn copy_file(from: &Path, to: &Path) -> Result<()> {
    create_private_dirs(to.parent().ok_or("missing destination parent")?)?;
    write_file(
        to,
        &fs::read(from)?,
        fs::metadata(from)?.permissions().mode() & 0o777,
    )
}
#[expect(
    clippy::disallowed_methods,
    reason = "The harness source-copy owner lists the package source tree and uses link metadata to reject unexpected source types."
)]
pub fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    let info = fs::symlink_metadata(from)?;
    if info.is_dir() {
        create_private_dirs(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else if info.is_file() {
        copy_file(from, to)?;
    } else {
        return Err(format!("unexpected source type: {}", from.display()).into());
    }
    Ok(())
}
#[expect(
    clippy::disallowed_methods,
    reason = "The source manifest owner lists the package source tree and distinguishes files and directories without following source links."
)]
fn bindings(root: &Path, path: &Path, output: &mut Vec<Binding>) -> Result<()> {
    let info = fs::symlink_metadata(path)?;
    if info.is_dir() {
        for entry in fs::read_dir(path)? {
            bindings(root, &entry?.path(), output)?;
        }
    } else if info.is_file() {
        output.push(hash_file(
            path,
            &path.strip_prefix(root)?.to_string_lossy(),
        )?);
    } else {
        return Err(format!("unexpected source type: {}", path.display()).into());
    }
    Ok(())
}
pub fn source_bindings(root: &Path) -> Result<Vec<Binding>> {
    let mut output = Vec::new();
    for name in [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "VERSION",
        "Makefile",
        "bin/agent-guard",
        "src",
        "examples",
        "cmd",
        "tests/harness",
    ] {
        bindings(root, &root.join(name), &mut output)?;
    }
    for name in ["go.mod", "go.sum"] {
        if root.join(name).try_exists()? {
            bindings(root, &root.join(name), &mut output)?;
        }
    }
    output.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(output)
}
pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    write_file(path, &bytes, 0o600)
}
pub fn json_line(file: &mut File, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}
pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
pub fn clean(path: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                output.pop();
            }
            Component::CurDir => {}
            other => output.push(other.as_os_str()),
        }
    }
    output
}
#[expect(
    clippy::disallowed_methods,
    reason = "The output-path owner resolves links and Git markers only after its lexical protected-path checks."
)]
pub fn outside_path(path: &Path) -> Result<PathBuf> {
    let absolute = clean(&if path.is_absolute() {
        path.into()
    } else {
        env::current_dir()?.join(path)
    });
    let home = PathBuf::from(env::var_os("HOME").ok_or("HOME is required")?);
    let mut parts: std::collections::VecDeque<_> = absolute
        .components()
        .filter_map(|c| match c {
            Component::Normal(value) => Some(value.to_os_string()),
            _ => None,
        })
        .collect();
    let mut resolved = PathBuf::from("/");
    let mut links = 0;
    while let Some(part) = parts.pop_front() {
        resolved.push(part);
        for name in [
            "Library",
            ".ssh",
            ".gnupg",
            ".aws",
            ".azure",
            ".config/gcloud",
        ] {
            if resolved.starts_with(home.join(name)) {
                return Err("output must stay outside protected paths".into());
            }
        }
        match fs::symlink_metadata(&resolved) {
            Ok(info) if info.file_type().is_symlink() => {
                links += 1;
                if links > 40 {
                    return Err("too many output symlinks".into());
                }
                let target = fs::read_link(&resolved)?;
                let target = clean(&if target.is_absolute() {
                    target
                } else {
                    resolved
                        .parent()
                        .ok_or("missing symlink parent")?
                        .join(target)
                });
                let mut next: std::collections::VecDeque<_> = target
                    .components()
                    .filter_map(|c| match c {
                        Component::Normal(value) => Some(value.to_os_string()),
                        _ => None,
                    })
                    .collect();
                next.append(&mut parts);
                parts = next;
                resolved = PathBuf::from("/");
                continue;
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        match fs::symlink_metadata(resolved.join(".git")) {
            Ok(_) => return Err("output must stay outside Git checkouts".into()),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(resolved)
}
pub fn new_output(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err("a new output directory is required".into());
    }
    let output = outside_path(path)?;
    create_private_dirs(output.parent().ok_or("missing output parent")?)?;
    fs::DirBuilder::new().mode(0o700).create(&output)?;
    Ok(output)
}
pub fn temporary(parent: &Path, prefix: &str) -> Result<PathBuf> {
    for n in 0..1000 {
        let path = parent.join(format!(
            "{prefix}{}-{}-{n}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        match fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => {
                return Ok(path);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    Err("could not create temporary directory".into())
}
#[expect(
    clippy::disallowed_methods,
    reason = "The runtime executable lookup owner checks PATH candidates for file type and execute permission."
)]
pub fn look_path(name: &str) -> Result<PathBuf> {
    if name.contains('/') {
        let path = PathBuf::from(name);
        if path.try_exists()? {
            return Ok(path);
        }
    } else {
        for root in env::split_paths(&env::var_os("PATH").unwrap_or_default()) {
            let path = root.join(name);
            if let Ok(info) = fs::metadata(&path)
                && info.is_file()
                && info.permissions().mode() & 0o111 != 0
            {
                return Ok(path);
            }
        }
    }
    Err(format!("exec: {name:?}: executable file not found in $PATH").into())
}
