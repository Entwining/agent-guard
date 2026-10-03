#![allow(dead_code)]
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
            "a59ddafbc4557a901670a7e7569295257d2c37df2fb92d54c08895d24883c4e6",
        ),
        (
            "tests/fixtures/rust-d22-scope.jsonl",
            "a3e41fd04bf1daf575c7312f9a275d298203ac7493228f6517dbcbb1fccd18ac",
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

// D22 and D10 rule 4 own this table. The frozen Go observer selects effective
// programs only; neither expected verdicts nor Rust parser output select scope.
fn outside_slice(row: &Value, scope: &BTreeMap<String, Value>) -> Option<String> {
    if row["tool"] != "Bash" {
        return None;
    }
    let id = format!("{}[{}]", row["family"].as_str().unwrap(), row["index"]);
    let selected = &scope[&id];
    let programs = selected["programs"].as_array().unwrap();
    let modelled = [
        "cat",
        "head",
        "tail",
        "less",
        "more",
        "bat",
        "sort",
        "uniq",
        "cut",
        "nl",
        "base64",
        "xxd",
        "od",
        "strings",
        "rg",
        "grep",
        "ag",
        "ack",
        "fd",
        "tree",
        "ls",
        "du",
        "find",
        "tar",
        "git",
        "printf",
        "echo",
        "print",
        "set",
        "declare",
        "typeset",
        "printenv",
        "export",
        "env",
        "xargs",
        "command",
        "exec",
        "nohup",
        "timeout",
        "nice",
        "sudo",
        "doas",
        "sh",
        "bash",
        "zsh",
        "dash",
        "ksh",
        "csh",
        "tcsh",
        "eval",
        "source",
        ".",
        "python",
        "python3",
        "node",
        "bun",
        "ruby",
        "perl",
        "php",
        "osascript",
        "lua",
        "deno",
        "true",
        "false",
        ":",
        "cd",
        "unset",
        "local",
        "setopt",
        "unsetopt",
        "emulate",
    ];
    if programs.is_empty()
        || programs.iter().any(|program| {
            let name = program.as_str().unwrap();
            modelled.contains(&name)
                || ["python", "node", "ruby", "perl", "php", "lua"]
                    .iter()
                    .any(|base| {
                        name.strip_prefix(base).is_some_and(|suffix| {
                            !suffix.is_empty()
                                && suffix.chars().all(|c| c.is_ascii_digit() || c == '.')
                        })
                    })
        })
    {
        return None;
    }
    Some(format!(
        "unmodelled programs: {}",
        programs
            .iter()
            .map(|p| p.as_str().unwrap())
            .collect::<Vec<_>>()
            .join(", ")
    ))
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
    report_rows(arm, None)
}

pub fn selected_report(arm: Arm, ids: &[&str]) -> Vec<Value> {
    report_rows(arm, Some(ids))
}

fn report_rows(arm: Arm, selected: Option<&[&str]>) -> Vec<Value> {
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
    let scope: BTreeMap<String, Value> = include_str!("../fixtures/rust-d22-scope.jsonl")
        .lines()
        .map(|line| {
            let row: Value = serde_json::from_str(line).unwrap();
            (row["id"].as_str().unwrap().to_owned(), row)
        })
        .collect();
    assert_eq!(scope.len(), 1265);
    for item in overlay {
        let id = item["id"].as_str().unwrap();
        if selected.is_some_and(|ids| !ids.contains(&id)) {
            continue;
        }
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
        let outside = outside_slice(row, &scope);
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
                user: Some("fixture-user".into()),
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
                    changed_contract_match &=
                        match contract["expected_coverage"]["error_kind"].as_str() {
                            Some("MalformedInput") => {
                                wire.exit == 2
                                    && wire.stderr.contains("invalid event input")
                                    && wire.stderr.contains("recheck")
                            }
                            Some("ProbeFault") => {
                                wire.stderr.contains("non-sensitive probe prefix")
                                    && wire.stderr.contains("repair")
                                    && wire.stderr.contains("recheck")
                            }
                            _ => false,
                        };
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
                                expected_commands.script.commands == commands.script.commands;
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
            observations.push(json!({"consumer":name,"expected":expected,"actual":actual_class,"coverage":coverage(&actual),"category":category,"permission_match":permission_match,"reason_match":reason_match,"advice_match":advice_match,"changed_contract_match":changed_contract_match,"recovery":recovery,"baseline_reason_partition":reason_kind,"go_deny_rust_permit":!old_reason.is_empty() && ["N","A","UC"].contains(&actual_class),"exit":wire.exit,"stdout":wire.stdout,"stderr":wire.stderr,"probe_count":probe.calls.len()}));
        }
        report.push(json!({"id":id,"family":row["family"],"arm":format!("{arm:?}"),"verdict":item["verdict"],"rule_id":item["rule_id"],"scope":outside,"observations":observations}));
    }
    let expected = selected.map_or(1289, <[&str]>::len);
    assert_eq!(ids.len(), expected);
    assert_eq!(report.len(), expected);
    report
}
