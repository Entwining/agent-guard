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

#[test]
fn built_package_passes_each_registered_protocol_case() {
    let root = root();
    let entry = package(root.path(), "none");
    fs::write(
        root.path().join("package/VERSION"),
        include_bytes!("../VERSION"),
    )
    .unwrap();
    write_executable(&entry, include_bytes!("../bin/agent-guard"));
    fs::copy(
        env!("CARGO_BIN_EXE_agent-guard-native"),
        root.path().join("package/bin/agent-guard-native"),
    )
    .unwrap();
    assert_protocol(&verifier(root.path(), &entry).output().unwrap());
    assert_clean(root.path());
}

#[test]
fn registered_events_use_runtime_names_and_top_level_cwd() {
    let root = root();
    let entry = package(root.path(), "event-shape");
    assert_protocol(&verifier(root.path(), &entry).output().unwrap());
    assert_clean(root.path());
}

#[test]
fn protocol_faults_are_reported_as_failed_acceptance() {
    for (fault, mismatch) in [
        ("all-allow", "expected exit 2"),
        ("all-deny", "expected exit 0"),
        ("empty-reason", "empty denial reason"),
        ("wrong-reason", "wrong denial reason"),
        ("status", "expected exit 2"),
        ("denial-stdout", "denial emitted advice"),
        ("missing-advice", "invalid Claude advice JSON"),
        ("changed-advice", "missing Claude advice"),
        ("non-claude-advice", "unexpected advice on allow"),
        ("missing-prefix", "missing Claude denial prefix"),
        ("allow-stderr", "unexpected stderr on allow"),
        ("advice-scalar", "invalid Claude advice JSON"),
        ("advice-field-type", "invalid Claude advice JSON"),
    ] {
        let root = root();
        let entry = package(root.path(), fault);
        let output = verifier(root.path(), &entry).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{fault}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(mismatch),
            "{fault}: {output:?}"
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("FAIL\t"));
        assert_clean(root.path());
    }
}

#[test]
fn advice_decoding_preserves_folded_names_repeated_fields_and_nulls() {
    for fault in ["advice-duplicate", "advice-case-fold"] {
        let root = root();
        let entry = package(root.path(), fault);
        assert_protocol(&verifier(root.path(), &entry).output().unwrap());
        assert_clean(root.path());
    }
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "This integration test resolves its synthetic installed entry alias for the identity assertion."
)]
fn installation_resolves_aliases_and_binds_both_executable_hashes() {
    let root = root();
    let entry = package(root.path(), "none");
    let alias = root.path().join("alias");
    symlink(&entry, &alias).unwrap();
    let output = verifier(root.path(), &alias).output().unwrap();
    assert_protocol(&output);
    let out = String::from_utf8(output.stdout).unwrap();
    assert!(out.starts_with(&format!(
        "Executable: {}\n",
        fs::canonicalize(&entry).unwrap().display()
    )));
    for (label, path) in [
        ("Entry", entry),
        ("Native", root.path().join("package/bin/agent-guard-native")),
    ] {
        assert!(out.contains(&format!(
            "{label} SHA256: {:x}\n",
            Sha256::digest(fs::read(path).unwrap())
        )));
    }
}

#[test]
fn default_executable_lookup_requires_a_path_entry_and_keeps_absolute_identity() {
    let root = root();
    let entry = package(root.path(), "none");
    let bin = entry.parent().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-guard-verify"));
    command
        .env_clear()
        .env("HOME", root.path())
        .env("TMPDIR", root.path())
        .env("PATH", bin);
    assert_protocol(&command.output().unwrap());
    command.env("PATH", "").current_dir(bin);
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "FAIL\tsetup\tmissing installed executable: exec: \"agent-guard\": executable file not found in $PATH\n"
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn invalid_and_incomplete_installations_fail_before_acceptance() {
    for fault in [
        "relative",
        "missing",
        "undeclared",
        "missing-native",
        "missing-version",
        "invalid-version",
        "not-executable",
        "arguments",
    ] {
        let root = root();
        let mut entry = package(root.path(), "none");
        match fault {
            "relative" => entry = PathBuf::from("relative"),
            "missing" => entry = root.path().join("missing"),
            "undeclared" => {
                let other = entry.with_file_name("undeclared");
                fs::copy(&entry, &other).unwrap();
                entry = other;
            }
            "missing-native" => {
                fs::remove_file(entry.with_file_name("agent-guard-native")).unwrap()
            }
            "missing-version" => fs::remove_file(root.path().join("package/VERSION")).unwrap(),
            "invalid-version" => {
                fs::write(root.path().join("package/VERSION"), "development\n").unwrap()
            }
            "not-executable" => {
                fs::set_permissions(&entry, fs::Permissions::from_mode(0o600)).unwrap()
            }
            "arguments" => {}
            _ => unreachable!(),
        }
        let mut command = verifier(root.path(), &entry);
        if fault == "arguments" {
            command.arg("extra");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{fault}: {output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).starts_with("FAIL\tsetup\t"));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn version_mismatch_fails_before_protocol_and_removes_temporary_home() {
    let root = root();
    let entry = package(root.path(), "none");
    write_executable(
        &entry.with_file_name("agent-guard-native"),
        b"#!/bin/sh\nprintf 'agent-guard 0.0.1\\n'\n",
    );
    let output = verifier(root.path(), &entry).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not match VERSION"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("RESULT\t"));
    assert_clean(root.path());
}

fn fixture_dead(root: &Path) {
    let ids = fs::read_to_string(root.join("fixture-pids")).unwrap();
    assert!(!ids.trim().is_empty());
    for raw in ids.split_whitespace() {
        let pid = Pid::from_raw(raw.parse().unwrap());
        let deadline = Instant::now() + Duration::from_secs(1);
        while kill(pid, None).is_ok() {
            if Instant::now() >= deadline {
                let _ = kill(pid, Signal::SIGKILL);
                panic!("fixture process {raw} survived");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn fixture_elapsed(root: &Path) -> Duration {
    let started: u128 = fs::read_to_string(root.join("fixture-started"))
        .unwrap()
        .parse()
        .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    Duration::from_nanos(u64::try_from(now - started).unwrap())
}

#[test]
fn deadline_kills_descendants_and_removes_temporary_home() {
    let root = root();
    let entry = package(root.path(), "hang");
    let output = verifier(root.path(), &entry).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("deadline exceeded"), "{output:?}");
    assert!(out.contains(&format!("1/{} completed", protocol_cases().len())));
    assert!(fixture_elapsed(root.path()) < Duration::from_secs(6));
    fixture_dead(root.path());
    assert_clean(root.path());
}

#[test]
fn exited_child_with_inherited_output_pipes_is_bounded_and_cleaned() {
    let root = root();
    let entry = package(root.path(), "orphan-pipe");
    let output = verifier(root.path(), &entry).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("WaitDelay expired before I/O complete")
    );
    assert!(fixture_elapsed(root.path()) < Duration::from_secs(2));
    fixture_dead(root.path());
    assert_clean(root.path());
}

#[test]
fn interrupts_preserve_signal_exit_and_clean_up_child_group() {
    for (signal, code) in [
        (Signal::SIGTERM, 143),
        (Signal::SIGINT, 130),
        (Signal::SIGHUP, 129),
    ] {
        let root = root();
        let entry = package(root.path(), "hang");
        let mut child = verifier(root.path(), &entry)
            .process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = Pid::from_raw(child.id() as i32);
        let deadline = Instant::now() + Duration::from_secs(8);
        while !root.path().join("fixture-pids").exists() {
            if Instant::now() >= deadline {
                let _ = killpg(pid, Signal::SIGKILL);
                child.wait().unwrap();
                panic!("fixture never became ready");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if signal == Signal::SIGINT {
            killpg(pid, signal).unwrap();
        } else {
            kill(pid, signal).unwrap();
        }
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = killpg(pid, Signal::SIGKILL);
                child.wait().unwrap();
                panic!("interrupted verifier did not finish");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(code), "{output:?}");
        fixture_dead(root.path());
        assert_clean(root.path());
        let text = String::from_utf8(output.stdout).unwrap();
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            text.contains("FAIL\tclaude\tproject-shell\t0\t-1\t; signal killed; expected exit 0\n"),
            "stdout:\n{text}stderr:\n{error}"
        );
        assert!(
            text.ends_with(&format!(
                "Summary: 0 pass, 1 fail; 1/{} completed\n",
                protocol_cases().len()
            )),
            "stdout:\n{text}stderr:\n{error}"
        );
        assert_eq!(error, "FAIL\tacceptance interrupted or deadline exceeded\n");
    }
}

#[test]
fn interruption_after_direct_child_exit_keeps_the_result_and_cleans_descendants() {
    let root = root();
    let entry = package(root.path(), "orphan-pipe");
    let mut child = verifier(root.path(), &entry)
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = Pid::from_raw(child.id() as i32);
    let deadline = Instant::now() + Duration::from_secs(8);
    let direct = loop {
        if let Ok(ids) = fs::read_to_string(root.path().join("fixture-pids")) {
            break Pid::from_raw(ids.split_whitespace().next().unwrap().parse().unwrap());
        }
        if Instant::now() >= deadline {
            let _ = killpg(pid, Signal::SIGKILL);
            child.wait().unwrap();
            panic!("fixture never became ready");
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    while kill(direct, None).is_ok() {
        assert!(Instant::now() < deadline, "direct child was not reaped");
        std::thread::sleep(Duration::from_millis(2));
    }
    kill(pid, Signal::SIGTERM).unwrap();
    let output = child.wait_with_output().unwrap();
    fixture_dead(root.path());
    assert_clean(root.path());
    assert_eq!(output.status.code(), Some(143), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        text.contains("PASS\tclaude\tproject-shell\t0\t0\t-\n"),
        "stdout:\n{text}stderr:\n{error}"
    );
    assert!(
        text.ends_with(&format!(
            "Summary: 1 pass, 0 fail; 1/{} completed\n",
            protocol_cases().len()
        )),
        "stdout:\n{text}stderr:\n{error}"
    );
    assert_eq!(error, "FAIL\tacceptance interrupted or deadline exceeded\n");
}

#[test]
fn later_interrupts_do_not_replace_the_first_signal_exit() {
    let root = root();
    let entry = package(root.path(), "signal-burst");
    let mut child = verifier(root.path(), &entry)
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = Pid::from_raw(child.id() as i32);
    let deadline = Instant::now() + Duration::from_secs(8);
    while !root.path().join("fixture-pids").exists() {
        if Instant::now() >= deadline {
            let _ = killpg(pid, Signal::SIGKILL);
            child.wait().unwrap();
            panic!("fixture never became ready");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    kill(pid, Signal::SIGTERM).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    kill(pid, Signal::SIGINT).unwrap();
    let output = child.wait_with_output().unwrap();
    fixture_dead(root.path());
    assert_clean(root.path());
    assert_eq!(output.status.code(), Some(143), "{output:?}");
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "This integration test checks the generated synthetic profile's private permission mode."
)]
fn profile_keeps_selected_identity_denial_uuids_permissions_and_existing_bytes() {
    for (kind, identifier) in [
        ("bundleID", "com.example.canary"),
        ("path", "/Applications/Canary & \"Test\"/client"),
    ] {
        let root = root();
        let output = root.path().join("appdata.mobileconfig");
        let requirement = "identifier \"com.example.canary\" and anchor apple";
        let args = [kind, identifier, requirement, output.to_str().unwrap()].map(String::from);
        let mut printed = Vec::new();
        profile::generate(&args, &mut printed).unwrap();
        let converted = Command::new("/usr/bin/plutil")
            .args(["-convert", "json", "-o", "-"])
            .arg(&output)
            .output()
            .unwrap();
        assert!(converted.status.success());
        let value: serde_json::Value = serde_json::from_slice(&converted.stdout).unwrap();
        assert_eq!(value["PayloadType"], "Configuration");
        assert_eq!(value["PayloadScope"], "System");
        let content = value["PayloadContent"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        let payload = &content[0];
        assert_eq!(
            payload["PayloadType"],
            "com.apple.TCC.configuration-profile-policy"
        );
        assert_eq!(payload["Services"].as_object().unwrap().len(), 1);
        let rows = payload["Services"]["SystemPolicyAppData"]
            .as_array()
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["Identifier"], identifier);
        assert_eq!(rows[0]["IdentifierType"], kind);
        assert_eq!(rows[0]["CodeRequirement"], requirement);
        assert_eq!(rows[0]["Allowed"], false);
        let uuid = |text: &str| {
            let parts = text.split('-').collect::<Vec<_>>();
            assert_eq!(
                parts.iter().map(|part| part.len()).collect::<Vec<_>>(),
                [8, 4, 4, 4, 12]
            );
            assert!(
                parts
                    .iter()
                    .all(|part| part.bytes().all(|byte| byte.is_ascii_hexdigit()))
            );
            assert!(parts[2].starts_with('4'));
            assert!(matches!(parts[3].as_bytes()[0], b'8' | b'9' | b'A' | b'B'));
        };
        uuid(value["PayloadUUID"].as_str().unwrap());
        uuid(payload["PayloadUUID"].as_str().unwrap());
        assert_ne!(value["PayloadUUID"], payload["PayloadUUID"]);
        assert_eq!(
            payload["PayloadIdentifier"],
            format!("{}.pppc", value["PayloadIdentifier"].as_str().unwrap())
        );
        assert_eq!(
            fs::metadata(&output).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let printed = String::from_utf8(printed).unwrap();
        for text in [
            "Plist validation: PASS",
            "Runtime enforcement: unverified",
            "A supervised Mac running macOS 14 or newer, admin rights",
            "baseline must open successfully",
            "Manual profile installation",
            "Allowed=false",
        ] {
            assert!(printed.contains(text));
        }
        let before = fs::read(&output).unwrap();
        assert!(profile::generate(&args, &mut Vec::new()).is_err());
        assert_eq!(fs::read(&output).unwrap(), before);
    }
}

#[test]
fn invalid_profiles_write_nothing() {
    let root = root();
    let output = root.path().join("invalid.mobileconfig");
    let output = output.to_str().unwrap();
    for args in [
        ["unknown", "com.example.canary", "anchor apple", output],
        ["path", "relative", "anchor apple", output],
        [
            "bundleID",
            "com.example.canary",
            "not a requirement",
            output,
        ],
        [
            "bundleID",
            "com.example.canary",
            "designated => identifier \"com.example.canary\"",
            output,
        ],
        ["bundleID", "com.example.canary", "anchor apple\n", output],
        ["bundleID", "com.example.canary\n", "anchor apple", output],
        ["bundleID", "invalid&identifier", "anchor apple", output],
        [
            "bundleID",
            "com.example.canary",
            "anchor apple",
            "relative.mobileconfig",
        ],
    ] {
        assert!(
            profile::generate(&args.map(String::from), &mut Vec::new()).is_err(),
            "{args:?}"
        );
        assert!(!Path::new(args[3]).exists());
    }
}

#[test]
fn profile_rejects_checkout_aliases_and_preserves_existing_symlink_target() {
    let root = root();
    let checkout = root.path().join("checkout");
    fs::create_dir(&checkout).unwrap();
    fs::write(checkout.join(".git"), "").unwrap();
    let alias = root.path().join("alias");
    symlink(&checkout, &alias).unwrap();
    for parent in [checkout, alias] {
        let output = parent.join("profile.mobileconfig");
        let args = [
            "bundleID",
            "com.example.canary",
            "anchor apple",
            output.to_str().unwrap(),
        ]
        .map(String::from);
        assert!(
            profile::generate(&args, &mut Vec::new())
                .unwrap_err()
                .contains("outside every checkout")
        );
    }
    let target = root.path().join("preserved");
    fs::write(&target, "preserved").unwrap();
    let link = root.path().join("profile.mobileconfig");
    symlink(&target, &link).unwrap();
    let args = [
        "bundleID",
        "com.example.canary",
        "anchor apple",
        link.to_str().unwrap(),
    ]
    .map(String::from);
    assert!(profile::generate(&args, &mut Vec::new()).is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "preserved");
}

#[test]
fn instructions_need_no_selected_client_and_cli_reports_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-guard-profile"))
        .arg("--instructions")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    for part in [
        "/usr/bin/profiles status -type enrollment",
        "baseline must open successfully",
        "cargo run --bin agent-guard-profile -- 'bundleID-or-path'",
        "enforcement remain unverified",
    ] {
        assert!(text.contains(part));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_agent-guard-profile"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("FAIL: expected identifier type"));
}

#[test]
fn written_profile_validation_failure_retains_the_artifact() {
    let root = root();
    let output = root.path().join("invalid.mobileconfig");
    assert!(
        profile::write_profile(&output, "not a plist")
            .unwrap_err()
            .contains("profile written but validation failed")
    );
    assert_eq!(fs::read_to_string(output).unwrap(), "not a plist");
}

#[test]
fn supervision_preserves_separate_and_combined_streams_without_pipe_deadlock() {
    use std::sync::atomic::AtomicUsize;
    for merged in [false, true] {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf out; printf err >&2"]);
        let result = public_tools::execute(
            &mut command,
            &[],
            Duration::from_secs(2),
            Duration::from_millis(250),
            &AtomicUsize::new(0),
            merged,
        )
        .unwrap();
        assert_eq!(result.exit, 0);
        assert!(!result.timed_out);
        assert_eq!(result.stdout, if merged { "outerr" } else { "out" });
        assert_eq!(result.stderr, if merged { "" } else { "err" });
    }
    let input = vec![b'a'; 128 * 1024];
    let result = public_tools::execute(
        &mut Command::new("/bin/cat"),
        &input,
        Duration::from_secs(2),
        Duration::from_millis(250),
        &AtomicUsize::new(0),
        false,
    )
    .unwrap();
    assert_eq!(result.exit, 0);
    assert!(!result.timed_out);
    assert_eq!(result.stdout.as_bytes(), input);
    assert!(result.stderr.is_empty());
}
