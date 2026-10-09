use super::*;

fn legacy_input<'a>(row: &'a Value, consumer: Consumer, fixture: &Fixture) -> (&'a str, Value) {
    let value = fixture.expand(row["input"].as_str().unwrap());
    match row["tool"].as_str().unwrap() {
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
    }
}

pub(super) fn observe_contract(
    fixture: &Fixture,
    row: &Value,
    item: &Value,
    outside: &Option<String>,
    arm: Arm,
    name: &str,
    consumer: Consumer,
) -> Value {
    let id = item["id"].as_str().unwrap();
    let (tool, input) = legacy_input(row, consumer, fixture);
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
    let mut probe = RecordingProbe::literal_for_quoted_paths(fixture);
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
    let effect = denial_effect(&actual);
    let rust_rule = denial_rule(&actual);
    let go_rule = go_reason_rule(old_reason);
    let same_denial_rule = go_rule != "None" && rust_rule == go_rule;
    let expected_text = if consumer == Consumer::Claude {
        format!(
            "DENIED: {old_reason} Do NOT bypass this restriction or retry the same blocked command.\n"
        )
    } else {
        format!("{old_reason}\n")
    };
    let consumer_text_match = wire.stderr == expected_text;
    let reason_match = if same_denial_rule {
        consumer_text_match
    } else if expected == "D" && item["verdict"] == "RETAIN" {
        denial_reason_matches(reason_kind, effect, actual_class, &wire.stderr, old_reason)
    } else {
        true
    };
    let advice_match = if expected == "A" {
        let output: Value = serde_json::from_str(&wire.stdout).unwrap_or(Value::Null);
        let message = output["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap_or("");
        !old["advice"].as_array().unwrap().is_empty()
            && old["advice"].as_array().unwrap().iter().all(|advice| {
                let expected = advice.as_str().unwrap();
                message.contains(expected)
            })
    } else {
        wire.stdout.is_empty()
    };
    let recovery = recovery(&actual);
    let changed_contract_match =
        changed_contract_matches(fixture, &context, item, &actual, &wire, name, arm);
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
    json!({"consumer":name,"expected":expected,"actual":actual_class,"coverage":coverage(&actual),"category":category,"permission_match":permission_match,"reason_match":reason_match,"same_denial_rule":same_denial_rule,"consumer_text_match":consumer_text_match,"go_rule":go_rule,"rust_rule":rust_rule,"advice_match":advice_match,"changed_contract_match":changed_contract_match,"recovery":recovery,"baseline_reason_partition":reason_kind,"go_deny_rust_permit":!old_reason.is_empty() && ["N","A","UC"].contains(&actual_class),"exit":wire.exit,"stdout":wire.stdout,"stderr":wire.stderr,"probe_count":probe.calls.len()})
}

fn changed_contract_matches(
    fixture: &Fixture,
    context: &Context,
    item: &Value,
    actual: &Result<agent_guard_rust::Evaluation, agent_guard_rust::CheckError>,
    wire: &agent_guard_rust::adapters::Wire,
    name: &str,
    arm: Arm,
) -> bool {
    let actual_class = class(actual);
    let effect = denial_effect(actual);
    let recovery = recovery(actual);
    let mut changed_contract_match = true;
    if item["verdict"] == "CHANGE" {
        let contract = &item["new_expectation"];
        let actual_coverage = coverage(actual);
        for (key, value) in contract["expected_coverage"].as_object().unwrap() {
            if key != "cause" {
                changed_contract_match &= actual_coverage[key] == *value;
            }
        }
        changed_contract_match &=
            changed_reason_matches(contract, wire, &recovery, actual_class, effect);
        if let Some(next) = contract["recovery_objective"]["next_operations"].get(name) {
            changed_contract_match &= recovery["next_step"]["kind"] == "owner_action";
            let got = fixture.expand_value(next);
            changed_contract_match &= recovery["automatic_application_supported"] == false;
            let recheck = serde_json::to_vec(
                &json!({"tool_name":got["tool"],"tool_input":got["input"],"cwd":got["cwd"]}),
            )
            .unwrap();
            let mut recheck_probe = RecordingProbe::literal_for_quoted_paths(fixture);
            changed_contract_match &= class(&evaluate_with_arm(
                Event {
                    bytes: &recheck,
                    context,
                    probe: &mut recheck_probe,
                },
                arm,
            )) == "N";
        }
    }
    changed_contract_match
}

fn denial_effect(
    actual: &Result<agent_guard_rust::Evaluation, agent_guard_rust::CheckError>,
) -> &str {
    match actual {
        Ok(e) => match &e.outcome {
            agent_guard_rust::Outcome::ProtectedDenial { reason, .. } => reason.effect.as_str(),
            _ => "",
        },
        Err(_) => "",
    }
}

fn recovery(actual: &Result<agent_guard_rust::Evaluation, agent_guard_rust::CheckError>) -> Value {
    match actual {
        Ok(e) => match &e.outcome {
            agent_guard_rust::Outcome::ProtectedDenial { recovery, .. }
            | agent_guard_rust::Outcome::CoverageInsufficient {
                recovery: Some(recovery),
                ..
            } => agent_guard_rust::adapters::recovery_value(recovery),
            _ => Value::Null,
        },
        Err(_) => Value::Null,
    }
}

fn changed_reason_matches(
    contract: &Value,
    wire: &agent_guard_rust::adapters::Wire,
    recovery: &Value,
    actual_class: &str,
    effect: &str,
) -> bool {
    let mut changed_contract_match = true;
    let excluded = recovery["excluded_scope"].to_string();
    changed_contract_match &= match contract["expected_coverage"]["cause"].as_str() {
        Some("identity_bound") => {
            wire.stderr.contains("ordinary absolute path")
                && excluded.contains("unresolved resource identity")
        }
        Some("inspection_budget") => {
            wire.stderr.contains("inspection budget")
                && excluded.contains("over-budget function expansion")
        }
        Some("unsupported_shell_syntax") => {
            wire.stderr.contains("shell syntax") && excluded.contains("original unsupported shell")
        }
        None => true,
        Some(_) => false,
    };
    changed_contract_match &= match contract["reason_contract"]["emission"].as_str() {
        Some("required") => !wire.stderr.is_empty() && [2, 3].contains(&wire.exit),
        Some("absent") => wire.stderr.is_empty(),
        _ => false,
    };
    changed_contract_match &= match contract["reason_contract"]["reason_class"].as_str() {
        Some("BroadRoot") => {
            effect.contains("broad recursive")
                && excluded.contains("outside")
                && excluded.contains("Library")
                && excluded.contains(".ssh")
                && excluded.contains("environment-file")
                && excluded.contains("whole-HOME task remains incomplete")
        }
        Some("Dump") => {
            effect.contains("environment dump")
                && excluded.contains("process environment dump")
                && excluded.contains("protected variable values")
        }
        Some("AppData") => effect.contains("App Data") && excluded.contains("Library"),
        Some("ProtectedCwd") => {
            effect.contains("protected cwd:")
                && excluded.contains("Library")
                && excluded.contains(".ssh")
                && match contract["reason_contract"]["protected_resource"].as_str() {
                    Some("AppData") => effect.contains("App Data"),
                    Some("SSH") => effect.contains("private-key"),
                    _ => false,
                }
        }
        _ => true,
    };
    if actual_class == "F" {
        changed_contract_match &= match contract["expected_coverage"]["error_kind"].as_str() {
            Some("MalformedInput") => {
                wire.exit == 2
                    && wire.stderr.contains("could not complete this check")
                    && wire.stderr.contains("recheck")
            }
            Some("ProbeFault") => {
                wire.stderr.contains("could not complete this check")
                    && wire.stderr.contains("repair")
                    && wire.stderr.contains("recheck")
            }
            _ => false,
        };
    }
    changed_contract_match
}

fn denial_rule(
    actual: &Result<agent_guard_rust::Evaluation, agent_guard_rust::CheckError>,
) -> String {
    match actual {
        Ok(e) => match &e.outcome {
            agent_guard_rust::Outcome::ProtectedDenial { reason, .. } => {
                format!("{:?}", reason.rule)
            }
            agent_guard_rust::Outcome::CoverageInsufficient {
                cause: agent_guard_rust::CoverageGap::UnsupportedShellSyntax,
                disposition: agent_guard_rust::Disposition::RejectUnsupportedSyntax,
                ..
            } => "Syntax".into(),
            _ => "None".into(),
        },
        Err(_) => "Fault".into(),
    }
}

fn denial_reason_matches(
    reason_kind: &str,
    effect: &str,
    actual_class: &str,
    stderr: &str,
    old_reason: &str,
) -> bool {
    match reason_kind {
        "AppData" => effect.contains("App Data") || effect.contains("broad recursive"),
        "credential_file" => {
            effect.contains("credential")
                || effect.contains("environment-file")
                || effect.contains("private-key")
        }
        "SSH" => effect.contains("private-key") || effect.contains("broad recursive"),
        "CodeFile" => effect.contains("CodeFile"),
        "hidden_content" => effect.contains("hidden"),
        "dump" => effect.contains("environment dump"),
        "variable" => effect.contains("credential variable"),
        "syntax" => actual_class == "UR",
        _ => stderr.contains(old_reason),
    }
}
