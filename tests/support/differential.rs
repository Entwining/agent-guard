use crate::support::{Fixture, RecordingProbe, class, coverage};
use agent_guard_rust::{
    Context, Event,
    adapters::{Consumer, render},
    evaluate_with_arm,
    shell::{self, Arm},
};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
};

pub fn validate_sources() {
    for (path, expected) in [
        (
            "tests/fixtures/contract.jsonl",
            "1e223c6453d6883acc88af9967beab4251ba0fc6d636a1186482b6e4b524c695",
        ),
        (
            "tests/fixtures/filesystem.json",
            "b1d51062925ccbfdfae5c8ffbc3e130e25391f31b05207448ee02be8cc874b3e",
        ),
        (
            "tests/fixtures/rust-contract-classification.jsonl",
            "af65478c60c1a997a01d1e0bd4bd00a174c6351a998eb1f62b800a5ccb4a27c4",
        ),
    ] {
        let output = Command::new("/usr/bin/shasum")
            .args(["-a", "256", path])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout)
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap(),
            expected,
            "frozen source drift: {path}"
        );
    }
}

fn fixture() -> Fixture {
    let fixture = Fixture::new();
    let setup: Value = serde_json::from_str(include_str!("../fixtures/filesystem.json")).unwrap();
    for directory in setup["directories"].as_array().unwrap() {
        fs::create_dir_all(format!("{}/{}", fixture.home, directory.as_str().unwrap())).unwrap();
    }
    for file in setup["files"].as_array().unwrap() {
        let path = format!("{}/{}", fixture.home, file.as_str().unwrap());
        fs::create_dir_all(Path::new(&path).parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }
    for link in setup["links"].as_array().unwrap() {
        let path = format!("{}/{}", fixture.home, link[0].as_str().unwrap());
        if fs::symlink_metadata(&path).is_ok() {
            fs::remove_file(&path).unwrap();
        }
        let target = fixture.expand(link[1].as_str().unwrap());
        std::os::unix::fs::symlink(target, path).unwrap();
    }
    let climb = format!(
        "{}{}{}",
        "../".repeat(fixture.project.split('/').count()),
        fixture.home.trim_start_matches('/'),
        "/project/data-link"
    );
    std::os::unix::fs::symlink(climb, format!("{}/firmlink-climb", fixture.project)).unwrap();
    fixture
}

// The selector uses only input mechanisms. Expectations never select their own denominator.
fn outside_slice(row: &Value, fixture: &Fixture) -> Option<String> {
    let input = row["input"].as_str().unwrap();
    if input.contains("<<")
        && (input.matches("<<").count() > 1
            || input.contains("0<<")
            || input.contains("';' .env")
            || input.split('\n').next().unwrap_or("").contains("; cat"))
    {
        return Some("complex heredoc fd/multiple-command arrangement outside the pinned two-parser dev subset".into());
    }
    if input.contains("$R") || input.contains("$U") || row["cwd"] == "" {
        return Some(
            "legacy username/parent or missing-cwd binding outside trusted P1 context".into(),
        );
    }
    if input.contains("/../")
        || input.ends_with("/..")
        || row["cwd"].as_str().unwrap().contains("/..")
    {
        return Some(
            "post-link parent traversal beyond the selected lexical/readlink slice".into(),
        );
    }
    if input.contains("/system/volumes/data") {
        return Some("case-folded firmlink spelling beyond canonical lexical normalization".into());
    }
    for link in [
        "public-link",
        "key-dir-link",
        "allowed-signers-link",
        "config-link",
    ] {
        if input.contains(link) {
            return Some(
                "SSH public-file alias identity requires deferred stat/inode owner".into(),
            );
        }
    }
    if input.contains(".ssh")
        && [".pub", "config", "known_hosts", "allowed_signers"]
            .iter()
            .any(|part| input.contains(part))
    {
        return Some("SSH public-file identity requires the deferred stat/inode owner".into());
    }
    if row["tool"] != "Bash" {
        if input.contains(['*', '?', '[', '{']) {
            return Some("API glob matching beyond the explicit-root slice".into());
        }
        let glob = row.get("glob").and_then(Value::as_str).unwrap_or("");
        if input != "$H"
            && !glob.starts_with('!')
            && !glob.starts_with(".env")
            && glob.contains(['*', '?', '[', '{'])
        {
            return Some("API glob matching beyond protected literal prefixes".into());
        }
        return None;
    }
    if input.contains("--include") || input.contains("\\|") {
        return Some("workflow advice beyond the P1 replacement partition".into());
    }
    if input.contains("$'") || input.contains("${") && !input.contains("${(") {
        return Some("parameter/ANSI-C expansion beyond the development word subset".into());
    }
    if input.split('$').skip(1).any(|part| {
        let name: String = part
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        ["TOKEN", "SECRET", "KEY", "PASSWORD", "CREDENTIAL"]
            .iter()
            .any(|key| name.to_ascii_uppercase().contains(key))
    }) {
        return Some(
            "credential-variable output partition beyond the development env-dump owner".into(),
        );
    }
    if input.contains("<<<")
        || input.contains(" | sh")
        || input.contains(" |& sh")
        || input.contains(" | env sh")
        || (input.contains(" | xargs")
            || input.contains(" |& xargs")
            || input.starts_with("xargs") && input.contains("<<"))
            && !input.contains("--files")
            && !input.trim_start().starts_with("printf x | xargs cat ")
    {
        return Some("stream-fed operand/code reconstruction beyond the dev name-list/content-consumer partition".into());
    }
    if input.contains("((") {
        return Some(
            "arithmetic command expansion beyond the development substitution partition".into(),
        );
    }
    if input.split('{').skip(1).any(|part| {
        part.split('}')
            .next()
            .is_some_and(|body| body.contains(',') || body.contains(".."))
    }) {
        return Some("brace expansion beyond the development word subset".into());
    }
    if input
        .split_whitespace()
        .next()
        .is_some_and(|word| word.contains('='))
        && input
            .split(';')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .count()
            > 1
    {
        return Some(
            "command-local/exported assignment scope beyond persistent dev assignments".into(),
        );
    }
    let source = fixture.expand(input);
    let observation =
        shell::observe(&source, Arm::Brush, &fixture.home, &fixture.project, true).unwrap();
    let supported = [
        "cat", "rg", "ls", "printf", "echo", "git", "python", "python3", "node", "bash", "zsh",
        "sh", "eval", "env", "printenv", "export", "true", "false", ":", "xargs",
    ];
    for command in observation.commands {
        if let Some(program) = command
            .argv
            .first()
            .map(|p| p.rsplit('/').next().unwrap_or(p))
        {
            if !supported.contains(&program) && program != "__observed_stream__" {
                return Some(format!("program adapter not in P1: {program}"));
            }
            if program == "git" && command.argv.get(1).is_none_or(|s| s != "commit") {
                return Some("Git adapter beyond literal commit-message data".into());
            }
            if ["python", "python3", "node"].contains(&program)
                && !command
                    .argv
                    .iter()
                    .any(|s| s == "-c" || s == "-e" || s == "--eval")
            {
                return Some(
                    "external interpreter input selection beyond the token partition".into(),
                );
            }
            if ["bash", "sh", "zsh"].contains(&program) && !command.argv.iter().any(|s| s == "-c") {
                return Some(
                    "shell script/stdin/wrapper invocation beyond explicit nested -c".into(),
                );
            }
            if program == "env"
                && !command.argv[1..].is_empty()
                && !command.argv[1..].iter().any(|s| s == "-i")
            {
                return Some("env wrapper/options beyond the frozen env -i dump partition".into());
            }
            if program == "export" && command.argv[0] != "export" {
                return Some("external executable named like a shell builtin".into());
            }
            if program == "export" && command.argv.iter().skip(1).any(|s| s.contains('=')) {
                return Some(
                    "exported-variable binding beyond persistent development assignments".into(),
                );
            }
            if command.unresolved
                && command
                    .argv
                    .iter()
                    .any(|s| s.contains("__observed_stream__/"))
            {
                return Some("substitution-generated target names beyond independent nested-effect observation".into());
            }
            if program == "cat"
                && (command
                    .argv
                    .iter()
                    .skip(1)
                    .any(|s| s == "~" || s == &fixture.home)
                    || command
                        .redirects
                        .iter()
                        .any(|(p, _)| p == "~" || p == &fixture.home))
            {
                return Some(
                    "directory input selection beyond named public/protected file reads".into(),
                );
            }
        }
    }
    if input.contains(['*', '?', '[']) && !input.contains("<<") {
        return Some(
            "legacy pattern expansion beyond explicit protected prefixes and dev globs".into(),
        );
    }
    None
}

fn reason_partition(reason: &str) -> &'static str {
    let reason = reason.to_lowercase();
    if reason.contains("symlink") {
        "identity"
    } else if reason.contains("inline code") {
        "CodeFile"
    } else if reason.contains("hidden files") {
        "hidden_content"
    } else if reason.contains("shell syntax") {
        "syntax"
    } else if reason.contains("dumps environment") {
        "dump"
    } else if reason.contains("variable") {
        "variable"
    } else if reason.contains("app-data") {
        "AppData"
    } else if reason.contains("credential or environment file") {
        "credential_file"
    } else if reason.contains(".ssh") {
        "SSH"
    } else {
        "other_secret_owner"
    }
}

pub fn report(arm: Arm) -> Vec<Value> {
    validate_sources();
    let legacy: Vec<Value> = include_str!("../fixtures/contract.jsonl")
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let overlay: Vec<Value> = include_str!("../fixtures/rust-contract-classification.jsonl")
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(legacy.len(), 1265);
    assert_eq!(overlay.len(), 1289);
    let legacy: BTreeMap<_, _> = legacy
        .into_iter()
        .map(|r| {
            (
                format!(
                    "{}[{}]",
                    r["family"].as_str().unwrap(),
                    r["index"].as_u64().unwrap()
                ),
                r,
            )
        })
        .collect();
    assert_eq!(legacy.len(), 1265);
    let mut ids = BTreeSet::new();
    let mut report = Vec::new();
    let fixture = fixture();
    for item in overlay {
        let id = item["id"].as_str().unwrap();
        assert!(ids.insert(id.to_owned()));
        assert_eq!(item["status"], "labelled");
        if item["kind"] == "filesystem_link" {
            let path = format!(
                "{}/{}",
                fixture.home,
                item["source"]["link_path"].as_str().unwrap()
            );
            let target = fs::read_link(path).unwrap().to_str().unwrap().to_owned();
            assert_eq!(
                target,
                fixture.expand(item["source"]["target"].as_str().unwrap())
            );
            report.push(json!({"id":id,"family":"filesystem_link","arm":format!("{arm:?}"),"status":"metadata_match","verdict":item["verdict"],"rule_id":item["rule_id"],"scope":"setup metadata; no operation verdict inferred"}));
            continue;
        }
        let row = &legacy[id];
        assert_eq!(item["source"]["family"], row["family"]);
        assert_eq!(item["source"]["index"], row["index"]);
        let outside = outside_slice(row, &fixture);
        let mut observations = Vec::new();
        for (name, consumer) in [
            ("claude", Consumer::Claude),
            ("codex", Consumer::Codex),
            ("pi", Consumer::Pi),
        ] {
            let value = fixture.expand(row["input"].as_str().unwrap());
            let (tool, input) = match row["tool"].as_str().unwrap() {
                "Bash" => (
                    if consumer == Consumer::Pi {
                        "bash"
                    } else {
                        "Bash"
                    },
                    json!({"command":value}),
                ),
                "Read" => (
                    if consumer == Consumer::Pi {
                        "read"
                    } else {
                        "Read"
                    },
                    if consumer == Consumer::Pi {
                        json!({"path":value})
                    } else {
                        json!({"file_path":value})
                    },
                ),
                "Write" | "Edit" => (
                    match (consumer, row["tool"].as_str().unwrap()) {
                        (Consumer::Pi, "Write") => "write",
                        (Consumer::Pi, _) => "edit",
                        (_, name) => name,
                    },
                    if consumer == Consumer::Pi {
                        json!({"path":value})
                    } else {
                        json!({"file_path":value})
                    },
                ),
                "Grep" => (
                    if consumer == Consumer::Pi {
                        "grep"
                    } else {
                        "Grep"
                    },
                    json!({"path":value,"pattern":"x","glob":row.get("glob").and_then(Value::as_str).unwrap_or("")}),
                ),
                _ => panic!("unexpected legacy tool"),
            };
            let context = Context {
                consumer,
                home: fixture.home.clone(),
                cwd: fixture.expand(row["cwd"].as_str().unwrap()),
                zsh_executor: consumer != Consumer::Pi,
                require_execution_owner: false,
                shell_observation_entries: Cell::new(0),
            };
            let body = serde_json::to_vec(&json!({"tool_name":tool,"tool_input":input})).unwrap();
            let mut probe = RecordingProbe::new(&fixture);
            if id == "appdata[115]" {
                probe.fault = Some(format!("{}/locked", fixture.project));
            }
            let actual = evaluate_with_arm(
                Event {
                    bytes: &body,
                    context: &context,
                    probe: &mut probe,
                },
                arm,
            );
            let wire = render(consumer, &actual);
            let old = &row["expected"][name];
            let old_reason = old["reason"].as_str().unwrap();
            let expected = if item["verdict"] == "CHANGE" {
                item["new_expectation"]["outcome_class"]
                    .as_str()
                    .unwrap()
                    .replace('-', "")
            } else if !old_reason.is_empty() {
                "D".into()
            } else if old["advice"].as_array().unwrap().is_empty() {
                "N".into()
            } else {
                "A".into()
            };
            let actual_class = class(&actual);
            let reason_kind = reason_partition(old_reason);
            let permission_match = actual_class == expected
                || expected == "N" && ["UC", "A"].contains(&actual_class)
                || expected == "D" && reason_kind == "syntax" && actual_class == "UR";
            let reason_match = if expected == "D" && item["verdict"] == "RETAIN" {
                match reason_kind {
                    "AppData" => {
                        wire.stderr.contains("App Data") || wire.stderr.contains("broad recursive")
                    }
                    "credential_file" => {
                        wire.stderr.contains("credential")
                            || wire.stderr.contains("environment-file")
                            || wire.stderr.contains("private-key")
                    }
                    "SSH" => {
                        wire.stderr.contains("private-key")
                            || wire.stderr.contains("broad recursive")
                    }
                    "CodeFile" => wire.stderr.contains("CodeFile"),
                    "hidden_content" => wire.stderr.contains("hidden"),
                    "dump" => wire.stderr.contains("environment dump"),
                    "variable" => wire.stderr.contains("credential variable"),
                    "syntax" => actual_class == "UR",
                    _ => false,
                }
            } else {
                true
            };
            let advice_match = if expected == "A" {
                wire.stdout.contains("-r replaces")
                    && old["advice"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|s| s.as_str().unwrap().contains("--replace"))
            } else {
                wire.stdout.is_empty()
            };
            let recovery = match &actual {
                Ok(e) => match &e.outcome {
                    agent_guard_rust::Outcome::ProtectedDenial { recovery, .. }
                    | agent_guard_rust::Outcome::CoverageInsufficient {
                        recovery: Some(recovery),
                        ..
                    } => agent_guard_rust::adapters::recovery_value(recovery),
                    _ => Value::Null,
                },
                Err(_) => Value::Null,
            };
            let mut changed_contract_match = true;
            if item["verdict"] == "CHANGE" {
                let contract = &item["new_expectation"];
                let actual_coverage = coverage(&actual);
                for (key, value) in contract["expected_coverage"].as_object().unwrap() {
                    if key != "cause" {
                        changed_contract_match &= actual_coverage[key] == *value;
                    }
                }
                let excluded = recovery["excluded_scope"].to_string();
                changed_contract_match &= match contract["expected_coverage"]["cause"].as_str() {
                    Some("identity_bound") => {
                        wire.stderr.contains("resource identity is unresolved")
                            && excluded.contains("unresolved resource identity")
                    }
                    Some("inspection_budget") => {
                        wire.stderr.contains("inspection budget")
                            && excluded.contains("over-budget function expansion")
                    }
                    Some("unsupported_shell_syntax") => {
                        wire.stderr.contains("complete shell input")
                            && excluded.contains("original unsupported shell")
                    }
                    None => true,
                    Some(_) => false,
                };
                changed_contract_match &= match contract["reason_contract"]["emission"].as_str() {
                    Some("required") => !wire.stderr.is_empty() && [2, 3].contains(&wire.exit),
                    Some("absent") => wire.stderr.is_empty(),
                    _ => false,
                };
                changed_contract_match &= match contract["reason_contract"]["reason_class"].as_str()
                {
                    Some("BroadRoot") => {
                        wire.stderr.contains("broad recursive")
                            && excluded.contains("outside")
                            && excluded.contains("Library")
                            && excluded.contains(".ssh")
                            && excluded.contains("environment-file")
                            && excluded.contains("whole-HOME task remains incomplete")
                    }
                    Some("Dump") => {
                        wire.stderr.contains("environment dump")
                            && excluded.contains("process environment dump")
                            && excluded.contains("protected variable values")
                    }
                    Some("AppData") => {
                        wire.stderr.contains("App Data") && excluded.contains("Library")
                    }
                    Some("ProtectedCwd") => {
                        wire.stderr.contains("protected cwd:")
                            && excluded.contains("Library")
                            && excluded.contains(".ssh")
                            && match contract["reason_contract"]["protected_resource"].as_str() {
                                Some("AppData") => wire.stderr.contains("App Data"),
                                Some("SSH") => wire.stderr.contains("private-key"),
                                _ => false,
                            }
                    }
                    _ => true,
                };
                if actual_class == "F" {
                    changed_contract_match &= wire.stderr.contains("non-sensitive probe prefix")
                        && wire.stderr.contains("repair")
                        && wire.stderr.contains("recheck");
                }
                if let Some(next) = contract["recovery_objective"]["next_operations"].get(name) {
                    changed_contract_match &= recovery["next_step"]["kind"] == "owner_action";
                    let got = fixture.expand_value(next);
                    changed_contract_match &= got["tool"] == next["tool"]
                        && got["cwd"] == fixture.expand_value(&next["cwd"]);
                    for (key, value) in next["input"].as_object().unwrap() {
                        if key == "command" {
                            let expected_commands = shell::observe(
                                &fixture.expand(value.as_str().unwrap()),
                                arm,
                                &fixture.home,
                                &fixture.project,
                                consumer != Consumer::Pi,
                            )
                            .unwrap();
                            let commands = shell::observe(
                                got["input"][key].as_str().unwrap(),
                                arm,
                                &fixture.home,
                                &fixture.project,
                                consumer != Consumer::Pi,
                            )
                            .unwrap();
                            changed_contract_match &=
                                expected_commands.commands == commands.commands;
                        } else {
                            changed_contract_match &=
                                got["input"][key] == fixture.expand_value(value);
                        }
                    }
                    changed_contract_match &= recovery["automatic_application_supported"] == false;
                    let recheck=serde_json::to_vec(&json!({"tool_name":got["tool"],"tool_input":got["input"],"cwd":got["cwd"]})).unwrap();
                    let mut recheck_probe = RecordingProbe::new(&fixture);
                    changed_contract_match &= class(&evaluate_with_arm(
                        Event {
                            bytes: &recheck,
                            context: &context,
                            probe: &mut recheck_probe,
                        },
                        arm,
                    )) == "N";
                }
            }
            let category = if let Some(reason) = &outside {
                format!("out_of_slice: {reason}")
            } else if permission_match && reason_match && advice_match && changed_contract_match {
                if item["verdict"] == "CHANGE" {
                    "intended_change".into()
                } else {
                    "match".into()
                }
            } else {
                "Rust_defect".into()
            };
            observations.push(json!({"consumer":name,"expected":expected,"actual":actual_class,"coverage":coverage(&actual),"category":category,"permission_match":permission_match,"reason_match":reason_match,"advice_match":advice_match,"changed_contract_match":changed_contract_match,"recovery":recovery,"baseline_reason_partition":reason_kind,"exit":wire.exit,"stdout":wire.stdout,"stderr":wire.stderr,"probe_count":probe.calls.len()}));
        }
        report.push(json!({"id":id,"family":row["family"],"arm":format!("{arm:?}"),"verdict":item["verdict"],"rule_id":item["rule_id"],"scope":outside,"observations":observations}));
    }
    assert_eq!(ids.len(), 1289);
    assert_eq!(report.len(), 1289);
    report
}
