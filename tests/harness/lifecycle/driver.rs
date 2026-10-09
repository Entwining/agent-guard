use super::*;

pub fn run_lifecycle(options: &LifecycleOptions) -> Result<()> {
    let faults = if options.control.is_empty() {
        FAULTS.to_vec()
    } else {
        vec![
            CONTROLS
                .iter()
                .find(|(name, _)| *name == options.control)
                .ok_or("unknown lifecycle control")?
                .1,
        ]
    };
    let output = new_output(&options.output)?;
    let bindings = source_bindings(&options.source)?;
    let build = output.join("test-build");
    for name in [
        "src",
        "tests",
        "examples",
        "cmd",
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "VERSION",
    ] {
        copy_tree(&options.source.join(name), &build.join(name))?;
    }
    let original = fs::read_to_string(options.source.join("src/entry.rs"))?;
    let entry = fs::read_to_string(options.source.join("bin/agent-guard"))?;
    let links = fs::read_to_string(options.source.join("src/filesystem/links.rs"))?;
    let cargo = look_path(&options.cargo)?;
    let home = PathBuf::from(env::var_os("HOME").ok_or("HOME is required")?);
    let cache = |name, directory| {
        env::var(name).unwrap_or_else(|_| home.join(directory).to_string_lossy().into())
    };
    let target = output.join("cargo-target");
    let environment = vec![
        (
            "PATH".into(),
            format!(
                "{}:/usr/bin:/bin",
                cargo.parent().ok_or("missing cargo parent")?.display()
            ),
        ),
        ("HOME".into(), build.to_string_lossy().into()),
        ("TMPDIR".into(), output.to_string_lossy().into()),
        ("CARGO_NET_OFFLINE".into(), "true".into()),
        ("CARGO_HOME".into(), cache("CARGO_HOME", ".cargo")),
        ("RUSTUP_HOME".into(), cache("RUSTUP_HOME", ".rustup")),
        ("CARGO_TARGET_DIR".into(), target.to_string_lossy().into()),
    ];
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(output.join("results.jsonl"))?;
    let (mut count, mut failed) = (0, 0);
    for fault in faults {
        let patch = inject_fault(&original, &entry, &links, fault, &options.control)?;
        let binary = build_fault(
            &patch,
            &build,
            &output,
            &cargo,
            &target,
            &environment,
            fault,
        )?;
        let binding = hash_file(&binary, "agent-guard-native")?;
        if matches!(fault, "large-reason" | "large-stderr") {
            verify_producer(&output, &binary, fault)?;
        }
        for repeat in 0..3 {
            let result = lifecycle_attempt(&output, &binary, &patch, fault, repeat)?;
            let problems = violations(fault, &result);
            count += 1;
            if !problems.is_empty() {
                failed += 1;
            }
            let mut row = serde_json::to_value(&result)?;
            row["fault"] = json!(fault);
            row["repeat"] = json!(repeat);
            row["violations"] = json!(problems);
            row["binary"] = serde_json::to_value(&binding)?;
            json_line(&mut file, &row)?;
            println!(
                "{fault} {repeat} status={} ms={:.1} violations=[{}]",
                result.process.status,
                result.process.milliseconds,
                problems.join(" ")
            );
        }
    }
    write_json(
        &output.join("summary.json"),
        &json!({"cases": count, "failed_cases": failed, "control": options.control, "source": bindings}),
    )?;
    if failed > 0 {
        return Err(format!("{failed} lifecycle cases violated their contract").into());
    }
    Ok(())
}

fn build_fault(
    patch: &FaultPatch,
    build: &Path,
    output: &Path,
    cargo: &Path,
    target: &Path,
    environment: &[(String, String)],
    fault: &str,
) -> Result<PathBuf> {
    for (name, text) in [
        ("src/entry.rs", &patch.runner),
        ("src/entry/lifecycle_fixture.rs", &patch.fixture),
        ("src/filesystem/links.rs", &patch.links),
    ] {
        create_private_dirs(build.join(name).parent().ok_or("missing fault parent")?)?;
        write_file(&build.join(name), text.as_bytes(), 0o600)?;
    }
    let binary = output.join(format!("fault-{fault}"));
    let compiled = run(
        &[
            cargo.to_string_lossy().into(),
            "build".into(),
            "--locked".into(),
            "--release".into(),
            "--bin".into(),
            "agent-guard-native".into(),
        ],
        &[],
        build,
        environment,
        Duration::ZERO,
    )?;
    write_json(&output.join(format!("{fault}-build.json")), &compiled)?;
    if compiled.status != 0 || compiled.timed_out {
        return Err(format!(
            "fault build failed: {} {}",
            compiled.spawn_error, compiled.stderr
        )
        .into());
    }
    copy_file(&target.join("release/agent-guard-native"), &binary)?;
    Ok(binary)
}

fn verify_producer(output: &Path, binary: &Path, fault: &str) -> Result<()> {
    let home = temporary(output, "producer-control-")?;
    let producer = run(
        &[binary.to_string_lossy().into(), "--checker".into()],
        b"{}",
        &home,
        &[
            ("HOME".into(), home.to_string_lossy().into()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ],
        Duration::from_secs(8),
    )?;
    let payload = if fault == "large-stderr" {
        &producer.stderr
    } else {
        &producer.stdout
    };
    let written = fs::read_to_string(home.join("producer-bytes"))?;
    write_json(&output.join(format!("{fault}-producer.json")), &producer)?;
    if producer.status != 2
        || payload != &format!("{}\n", "x".repeat(131072))
        || written != "131073"
    {
        return Err("independent producer control failed".into());
    }
    Ok(())
}

fn lifecycle_attempt(
    output: &Path,
    binary: &Path,
    patch: &FaultPatch,
    fault: &str,
    repeat: usize,
) -> Result<LifecycleResult> {
    let home = temporary(output, &format!("{fault}-{repeat}-"))?;
    let bin = home.join("package/bin");
    create_private_dirs(&bin)?;
    copy_file(binary, &bin.join("agent-guard-native"))?;
    write_file(&bin.join("agent-guard"), patch.entry.as_bytes(), 0o700)?;
    let body: &[u8] = if fault == "dependency-failure" {
        br#"{"tool_name":"Bash","tool_input":{"command":"true"}}"#
    } else {
        br#"{"marker":"synthetic input"}"#
    };
    let result = match execute_fault(&bin.join("agent-guard"), &home, body) {
        Ok(result) => result,
        Err(e) => {
            if let Some(failure) = e.downcast_ref::<LifecycleFailure>() {
                write_json(
                    &output.join(format!("{fault}-{repeat}-error.json")),
                    &failure.result,
                )?;
            }
            return Err(e);
        }
    };
    Ok(result)
}
