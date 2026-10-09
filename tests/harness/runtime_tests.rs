use super::*;

fn fixtures(home: &Path, guard: &[u8]) -> (PathBuf, PathBuf) {
    let bin = home.join("clients");
    fs::create_dir(&bin).unwrap();
    let helper = env!("CARGO_BIN_EXE_agent-guard-harness-client");
    for runtime in ["claude", "pi", "codex"] {
        #[expect(
            clippy::disallowed_methods,
            reason = "This fixture resolves only its synthetic client directory to test executable aliases."
        )]
        let client = if runtime == "codex" {
            fs::canonicalize(&bin).unwrap().join("installed-codex")
        } else {
            bin.join(runtime)
        };
        let check = if runtime == "codex" {
            format!(
                "test \"$0\" = {} || exit 80\n",
                quote(&client.to_string_lossy())
            )
        } else {
            String::new()
        };
        write_file(&client,format!("#!/bin/sh\n{check}if test \"$1\" = --version; then printf 'synthetic runtime 1\\n'; exit 0; fi\nexec {} {runtime} \"$@\"\n",quote(helper)).as_bytes(),0o700).unwrap();
        if runtime == "codex" {
            symlink(&client, bin.join(runtime)).unwrap();
        }
    }
    let package = home.join("package/bin");
    fs::create_dir_all(&package).unwrap();
    let entry = package.join("agent-guard");
    write_file(&entry, guard, 0o700).unwrap();
    write_file(
        &package.join("agent-guard-native"),
        b"synthetic binding only",
        0o700,
    )
    .unwrap();
    (bin, entry)
}
fn driver(
    bin: &Path,
    entry: &Path,
    output: &Path,
    ablate: bool,
    runtimes: &str,
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-guard-runtime"));
    command
        .args(["--source", env!("CARGO_MANIFEST_DIR"), "--entry"])
        .arg(entry)
        .arg("--output")
        .arg(output)
        .args(["--runtimes", runtimes])
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()));
    if ablate {
        command.arg("--ablate");
    }
    command.output().unwrap()
}
#[test]
fn runtime_driver_observes_all_cases_bindings_controls_and_missing_hooks() {
    let home = scratch("driver-");
    let (bin,entry) = fixtures(&home,b"#!/bin/sh\nbody=$(cat)\ncase \"$body\" in *Library*|*.ssh*|*find*) printf 'DENIED: synthetic rule\\n' >&2; exit 2;; esac\n");
    for name in ["missing", "other"] {
        let selected = entry.with_file_name(name);
        if name == "other" {
            write_file(&selected, b"#!/bin/sh\n", 0o700).unwrap();
        }
        let directory = home.join(format!("invalid-{name}"));
        assert!(
            !driver(&bin, &selected, &directory, false, "codex")
                .status
                .success()
        );
        assert!(!directory.exists());
    }
    let alias = home.join("entry-alias");
    symlink(&entry, &alias).unwrap();
    for ablate in [false, true] {
        let directory = home.join(format!("report-{ablate}"));
        let output = driver(
            &bin,
            if ablate { &entry } else { &alias },
            &directory,
            ablate,
            "claude,pi,codex",
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value =
            serde_json::from_slice(&fs::read(directory.join("report.json")).unwrap()).unwrap();
        assert_runtime_records(&report, ablate);
        assert_package_bindings(&report, &entry);
        let clients: Vec<RuntimeClient> =
            serde_json::from_value(report["clients"].clone()).unwrap();
        assert_eq!(clients.len(), 3);
        let mut seen = BTreeSet::new();
        for client in clients {
            assert!(seen.insert(client.runtime.clone()));
            #[expect(
                clippy::disallowed_methods,
                reason = "This assertion resolves a synthetic client alias to verify the recorded executable identity."
            )]
            let path = fs::canonicalize(bin.join(&client.runtime)).unwrap();
            let binding = hash_file(&path, &path.to_string_lossy()).unwrap();
            assert_eq!(
                (
                    client.executable.path,
                    client.executable.sha256,
                    client.executable.mode
                ),
                (binding.path, binding.sha256, binding.mode)
            );
            assert_eq!(client.version.status, 0);
            assert_eq!(client.version.stdout, "synthetic runtime 1\n");
        }
    }
    write_file(
        &bin.join("codex"),
        b"#!/bin/sh\nprintf 'SYNTHETIC_RUNTIME_CLIENT\\n'\nexit 7\n",
        0o700,
    )
    .unwrap();
    let directory = home.join("missing-hook");
    assert!(
        !driver(&bin, &entry, &directory, false, "codex")
            .status
            .success()
    );
    let report: Value =
        serde_json::from_slice(&fs::read(directory.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["records"].as_array().unwrap().len(), 1);
    assert_eq!(report["records"][0]["id"], CORPUS[0].id);
    assert_eq!(report["records"][0]["runtime"], "codex");
    assert_eq!(report["records"][0]["verdict"], "unverified");
    assert_eq!(report["summary"][0]["plannedCalls"], CORPUS.len() * 3);
    assert_eq!(report["summary"][0]["verified"], 0);
    assert!(
        report["summary"][0]["falseAllow"].is_null() && report["summary"][0]["falseDeny"].is_null()
    );
    fs::remove_dir_all(home).unwrap();
}

fn assert_runtime_records(report: &Value, ablate: bool) {
    let rows: Vec<RuntimeRow> = serde_json::from_value(report["records"].clone()).unwrap();
    let summaries: Vec<RuntimeSummary> = serde_json::from_value(report["summary"].clone()).unwrap();
    let mut expected = BTreeSet::new();
    for runtime in ["claude", "pi", "codex"] {
        for run in 0..3 {
            for case in CORPUS {
                expected.insert((runtime.to_owned(), run, case.id.to_owned()));
            }
        }
    }
    for row in rows {
        assert!(
            expected.remove(&(row.runtime, row.run, row.id.clone())),
            "duplicate or unknown case"
        );
        let case = CORPUS.iter().find(|case| case.id == row.id).unwrap();
        assert_eq!(row.command, case.command);
        assert_eq!(row.expected, case.expected);
        assert_eq!(
            row.verdict.runtime,
            if ablate { "allow" } else { case.expected }
        );
        assert_eq!(row.hooks.len(), 1);
        assert!(row.result.is_some());
        assert!(!row.verdict.conflict);
    }
    assert!(expected.is_empty());
    assert_eq!(summaries.len(), 3);
    for summary in summaries {
        assert!(summary.complete);
        assert_eq!(summary.verified, CORPUS.len() * 3);
        assert_eq!(summary.matched, CORPUS.len() * 3);
    }
}

fn assert_package_bindings(report: &Value, entry: &Path) {
    let bindings: Vec<Binding> = serde_json::from_value(report["manifest"].clone()).unwrap();
    assert_eq!(bindings.len(), 2);
    let mut names = BTreeSet::new();
    for binding in bindings {
        assert!(names.insert(binding.path.clone()));
        let expected = hash_file(
            &entry
                .parent()
                .unwrap()
                .join(Path::new(&binding.path).file_name().unwrap()),
            &binding.path,
        )
        .unwrap();
        assert_eq!(
            (binding.sha256, binding.mode),
            (expected.sha256, expected.mode)
        );
    }
    assert_eq!(
        names,
        BTreeSet::from(["bin/agent-guard".into(), "bin/agent-guard-native".into()])
    );
}
