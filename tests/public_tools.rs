#![forbid(unsafe_code)]

#[path = "../cmd/agent-guard-profile/profile.rs"]
mod profile;
#[path = "../cmd/public_tools.rs"]
mod public_tools;

use nix::{
    sys::signal::{Signal, kill, killpg},
    unistd::Pid,
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::{
        fs::{PermissionsExt, symlink},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("public-tools-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap()
}

fn write_executable(path: &Path, body: &[u8]) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
}

fn package(root: &Path, fault: &str) -> PathBuf {
    let bin = root.join("package/bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(root.join("package/VERSION"), "0.0.0\n").unwrap();
    write_executable(
        &bin.join("agent-guard-native"),
        b"#!/bin/sh\nprintf 'agent-guard 0.0.0\\n'\n",
    );
    let entry = bin.join("agent-guard");
    let fixture = Path::new(env!("CARGO_BIN_EXE_agent-guard-public-tool-fixture"));
    write_executable(
        &entry,
        format!("#!/bin/sh\nexec {} '{fault}' \"$@\"\n", quote(fixture)).as_bytes(),
    );
    entry
}

fn verifier(root: &Path, entry: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-guard-verify"));
    command
        .arg(entry)
        .env_clear()
        .env("HOME", root)
        .env("TMPDIR", root)
        .env("PATH", "/usr/bin:/bin");
    command
}

#[expect(
    clippy::disallowed_methods,
    reason = "The test lists its own temporary root to find leftover acceptance directories."
)]
fn assert_clean(root: &Path) {
    assert!(!fs::read_dir(root).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("agent-guard-acceptance-")
    }));
}

fn protocol_cases() -> std::collections::BTreeMap<(&'static str, &'static str), String> {
    let mut expected = std::collections::BTreeMap::new();
    for runtime in ["claude", "codex", "pi"] {
        for (name, shell, exit) in [
            ("project-shell", true, 0),
            ("project-read", false, 0),
            ("environment-write", false, 0),
            ("client-key-use", true, 0),
            ("public-key-read", false, 0),
            ("project-search", false, 0),
            ("claude-advice", true, 0),
            ("appdata-shell", true, 2),
            ("appdata-read", false, 2),
            ("appdata-link", true, 2),
            ("environment-read", true, 2),
            ("private-key-read", false, 2),
            ("broad-scan-advice", true, 2),
        ] {
            if runtime != "codex" || shell {
                expected.insert((runtime, name), exit.to_string());
            }
        }
    }
    expected
}

fn assert_protocol(output: &Output) {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let mut expected = protocol_cases();
    let planned = expected.len();
    let out = std::str::from_utf8(&output.stdout).unwrap();
    for line in out
        .lines()
        .filter(|line| line.starts_with("PASS\t") || line.starts_with("FAIL\t"))
    {
        let fields = line.splitn(6, '\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 6, "{line}");
        assert_eq!(fields[0], "PASS", "{line}");
        let exit = expected
            .remove(&(fields[1], fields[2]))
            .unwrap_or_else(|| panic!("unknown or duplicate protocol row"));
        assert_eq!(fields[3], exit, "{line}");
        assert_eq!(fields[4], exit, "{line}");
    }
    assert!(expected.is_empty(), "missing protocol rows: {expected:?}");
    assert!(out.contains(&format!(
        "Summary: {planned} pass, 0 fail; {planned}/{planned} completed"
    )));
}

#[path = "public_tools/lifecycle.rs"]
mod lifecycle;
#[path = "public_tools/profiles.rs"]
mod profiles;
#[path = "public_tools/protocol.rs"]
mod protocol;
#[path = "public_tools/streams.rs"]
mod streams;
