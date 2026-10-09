use std::{
    fs,
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use crate::public_tools::{ResultOutput, execute, path_error};
use sha2::{Digest, Sha256};

const USAGE: &str = "usage: agent-guard-verify [absolute-installed-executable]";
const ADVICE: &str = "rg -r means --replace. Drop -r; use -n for line numbers, or spell --replace VALUE for an intentional replacement.";

struct Installation {
    entry: PathBuf,
    native: PathBuf,
    version: String,
    entry_hash: String,
    native_hash: String,
}

#[expect(
    clippy::disallowed_methods,
    reason = "The installed executable owner checks file type and execute permissions before invocation."
)]
fn executable(path: &Path) -> Result<(), String> {
    let info = fs::metadata(path).map_err(|error| path_error("stat", path, &error))?;
    if !info.is_file() || info.permissions().mode() & 0o111 == 0 {
        return Err(format!("not an executable file: {}", path.display()));
    }
    Ok(())
}

fn installed(args: &[String]) -> Result<Installation, String> {
    if args.len() > 1 {
        return Err(USAGE.into());
    }
    let path = if let Some(path) = args.first() {
        PathBuf::from(path)
    } else {
        let paths = std::env::var_os("PATH").unwrap_or_default();
        let found = std::env::split_paths(&paths)
            .filter(|_| !paths.is_empty())
            .map(|directory| directory.join("agent-guard"))
            .find(|path| executable(path).is_ok())
            .ok_or("missing installed executable: exec: \"agent-guard\": executable file not found in $PATH")?;
        if !found.is_absolute() {
            return Err("missing installed executable: exec: \"agent-guard\": cannot run executable found relative to current directory".into());
        }
        found
    };
    if !path.is_absolute() {
        return Err(format!("missing absolute installed executable; {USAGE}"));
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "The installed entry owner resolves aliases before binding the selected package identity."
    )]
    let entry = fs::canonicalize(&path).map_err(|error| path_error("lstat", &path, &error))?;
    executable(&entry)?;
    let bin = entry.parent().ok_or("missing executable parent")?;
    if entry.file_name().is_none_or(|name| name != "agent-guard")
        || bin.file_name().is_none_or(|name| name != "bin")
    {
        return Err("selected entry is not the installed bin/agent-guard executable".into());
    }
    let version_path = bin
        .parent()
        .ok_or("missing package parent")?
        .join("VERSION");
    let version = String::from_utf8_lossy(
        &fs::read(&version_path).map_err(|error| path_error("open", &version_path, &error))?,
    )
    .trim()
    .to_owned();
    let parts = version.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts[0] != "0"
        || parts[1..]
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err("installed VERSION is missing or invalid".into());
    }
    let native_path = bin.join("agent-guard-native");
    #[expect(
        clippy::disallowed_methods,
        reason = "The installed native owner resolves its identity before hashing and invoking it."
    )]
    let native = fs::canonicalize(&native_path)
        .map_err(|error| path_error("lstat", &native_path, &error))?;
    executable(&native)?;
    let hash = |path: &Path| {
        fs::read(path)
            .map(|body| format!("{:x}", Sha256::digest(body)))
            .map_err(|error| path_error("open", path, &error))
    };
    let entry_hash = hash(&entry)?;
    let native_hash = hash(&native)?;
    Ok(Installation {
        entry,
        native,
        version,
        entry_hash,
        native_hash,
    })
}

mod contract;
use contract::{cases, mismatches};

fn verify(
    args: &[String],
    stdout: &mut dyn Write,
    interrupted: &AtomicUsize,
) -> Result<(), String> {
    let pkg = installed(args).map_err(|error| format!("setup\t{error}"))?;
    writeln!(
        stdout,
        "Executable: {}\nVersion: {}\nEntry SHA256: {}\nNative SHA256: {}",
        pkg.entry.display(),
        pkg.version,
        pkg.entry_hash,
        pkg.native_hash
    )
    .map_err(|error| error.to_string())?;
    let temporary = tempfile::Builder::new()
        .prefix("agent-guard-acceptance-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    #[expect(
        clippy::disallowed_methods,
        reason = "The acceptance fixture owner resolves only its newly created synthetic temporary root."
    )]
    let root = fs::canonicalize(temporary.path()).map_err(|error| error.to_string())?;
    let (home, cwd) = acceptance_home(&root)?;
    verify_versions(&pkg, &cwd, &home, &root, interrupted)?;
    let rows = cases(&home, &cwd);
    let planned = rows.len() * 2 + rows.iter().filter(|row| row.tool == "Bash").count();
    let mut completed = 0;
    let mut failures = 0;
    writeln!(
        stdout,
        "RESULT\tRUNTIME\tCASE\tEXPECTED\tACTUAL\tREASON / MISMATCH"
    )
    .map_err(|error| error.to_string())?;
    for runtime in ["claude", "codex", "pi"] {
        for row in &rows {
            if runtime == "codex" && row.tool != "Bash" {
                continue;
            }
            if interrupted.load(Ordering::Relaxed) != 0 {
                return Err("context canceled".into());
            }
            let tool = if runtime == "pi" {
                row.tool.to_lowercase()
            } else {
                row.tool.to_owned()
            };
            let event = serde_json::to_vec(
                &serde_json::json!({"tool_name":tool,"tool_input":row.input,"cwd":cwd}),
            )
            .map_err(|error| error.to_string())?;
            let result = execute_package(
                &pkg.entry,
                &["--runtime", runtime],
                &event,
                &cwd,
                &home,
                &root,
                interrupted,
            )?;
            let problems = mismatches(&result, row, runtime);
            let status = if problems.is_empty() {
                "PASS"
            } else {
                failures += 1;
                "FAIL"
            };
            completed += 1;
            let mut detail = vec![result.stderr.clone()];
            detail.extend(problems);
            let detail = detail
                .join("; ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let detail = if detail.is_empty() { "-" } else { &detail };
            writeln!(
                stdout,
                "{status}\t{runtime}\t{}\t{}\t{}\t{detail}",
                row.name, row.expected, result.exit
            )
            .map_err(|error| error.to_string())?;
            if result.timed_out || interrupted.load(Ordering::Relaxed) != 0 {
                writeln!(
                    stdout,
                    "Summary: {} pass, {failures} fail; {completed}/{planned} completed",
                    completed - failures
                )
                .map_err(|error| error.to_string())?;
                return Err("acceptance interrupted or deadline exceeded".into());
            }
        }
    }
    writeln!(
        stdout,
        "Summary: {} pass, {failures} fail; {completed}/{planned} completed",
        completed - failures
    )
    .map_err(|error| error.to_string())?;
    if failures != 0 {
        return Err("installed package acceptance failed".into());
    }
    Ok(())
}

pub fn main(args: &[String]) -> i32 {
    let interrupted = Arc::new(AtomicUsize::new(0));
    let mut signals = match signal_hook::iterator::Signals::new([
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
    ]) {
        Ok(signals) => signals,
        Err(error) => {
            eprintln!("FAIL\t{error}");
            return 1;
        }
    };
    let handle = signals.handle();
    let flag = Arc::clone(&interrupted);
    let listener = std::thread::spawn(move || {
        for signal in signals.forever() {
            let _ = flag.compare_exchange(
                0,
                128 + signal as usize,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
    });
    let code = match verify(args, &mut io::stdout(), &interrupted) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("FAIL\t{error}");
            1
        }
    };
    handle.close();
    if listener.join().is_err() {
        eprintln!("FAIL\tsignal listener failed");
        return 1;
    }
    let signal_code = interrupted.load(Ordering::Relaxed) as i32;
    if signal_code == 0 { code } else { signal_code }
}

fn acceptance_home(root: &Path) -> Result<(PathBuf, PathBuf), String> {
    let home = root.join("home");
    let cwd = home.join("project");
    for directory in [
        "home/.ssh",
        "home/Library/Containers/com.example.canary",
        "home/project/src",
    ] {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(root.join(directory))
            .map_err(|error| error.to_string())?;
    }
    for path in [
        ".ssh/id_rsa",
        ".ssh/id.pub",
        "project/.env",
        "project/file.txt",
        "Library/Containers/com.example.canary/file.txt",
    ] {
        let path = home.join(path);
        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|error| path_error("open", &path, &error))?;
        nix::unistd::close(file).map_err(|error| path_error("close", &path, &error.into()))?;
    }
    symlink(home.join("Library/Containers"), cwd.join("data-link"))
        .map_err(|error| error.to_string())?;
    Ok((home, cwd))
}

fn execute_package(
    path: &Path,
    args: &[&str],
    body: &[u8],
    cwd: &Path,
    home: &Path,
    root: &Path,
    interrupted: &AtomicUsize,
) -> Result<ResultOutput, String> {
    let mut command = Command::new(path);
    command
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .env("HOME", home)
        .env("TMPDIR", root)
        .env("PATH", "/usr/bin:/bin");
    execute(
        &mut command,
        body,
        Duration::from_millis(4500),
        Duration::from_millis(500),
        interrupted,
        false,
    )
}

fn verify_versions(
    pkg: &Installation,
    cwd: &Path,
    home: &Path,
    root: &Path,
    interrupted: &AtomicUsize,
) -> Result<(), String> {
    for path in [&pkg.native, &pkg.entry] {
        let result = execute_package(path, &["--version"], &[], cwd, home, root, interrupted)?;
        if result.timed_out
            || result.signal.is_some()
            || result.exit != 0
            || !result.stderr.is_empty()
            || result.stdout != format!("agent-guard {}\n", pkg.version)
        {
            return Err(format!(
                "installed version does not match VERSION: {}",
                path.display()
            ));
        }
    }
    Ok(())
}
