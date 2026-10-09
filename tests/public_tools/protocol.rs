use super::*;

#[test]
fn built_package_passes_each_registered_protocol_case() {
    let root = root();
    let entry = package(root.path(), "none");
    fs::write(
        root.path().join("package/VERSION"),
        include_bytes!("../../VERSION"),
    )
    .unwrap();
    write_executable(&entry, include_bytes!("../../bin/agent-guard"));
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
