mod support;
use agent_guard_rust::{
    Event, Outcome,
    adapters::{recovery_value, render},
    evaluate_with_arm,
    filesystem::DiskProbe,
    shell::Arm,
};
use serde_json::{Value, json};

fn check_group(group: &str) {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/rust-review-d22.json")).unwrap();
    let mut failures = Vec::new();
    for case in cases.iter().filter(|case| case["group"] == group) {
        for arm in [Arm::Brush, Arm::TreeSitter] {
            let fixture = support::Fixture::new();
            let home_alias = fixture.root.join("home-alias");
            std::os::unix::fs::symlink(&fixture.home, &home_alias).unwrap();
            use std::os::unix::ffi::OsStrExt;
            std::os::unix::fs::symlink(
                std::ffi::OsStr::from_bytes(b"bad-directory-\xff"),
                format!("{}/bad-link", fixture.project),
            )
            .unwrap();
            let consumer = case["consumer"].as_str().unwrap_or("codex");
            let tool =
                case["tool"]
                    .as_str()
                    .unwrap_or(if consumer == "pi" { "bash" } else { "Bash" });
            let mut context=fixture.context(&json!({"consumer":consumer,"cwd":case["cwd"].as_str().unwrap_or("$P"),"task_objective":"review input","tool":tool,"provenance_form":"event"}));
            if case["setup"] == "home-alias" {
                context.home = home_alias.to_str().unwrap().into();
            }
            let mut event=case.get("event").cloned().unwrap_or_else(|| json!({"tool_name":tool,"tool_input":case.get("input").cloned().unwrap_or_else(||json!({"command":case["command"].as_str().unwrap_or("")}))}));
            if let Some(cwd) = case.get("event_cwd") {
                event["cwd"] = cwd.clone();
            }
            let event = fixture.expand_value(&event);
            let bytes = case["raw"]
                .as_str()
                .map(|s| s.as_bytes().to_vec())
                .unwrap_or_else(|| serde_json::to_vec(&event).unwrap());
            let result = evaluate_with_arm(
                Event {
                    bytes: &bytes,
                    context: &context,
                    probe: &mut DiskProbe,
                },
                arm,
            );
            let wire = render(context.consumer, &result);
            let coverage = support::coverage(&result);
            let (reason, recovery) = match &result {
                Ok(e) => match &e.outcome {
                    Outcome::ProtectedDenial { reason, recovery } => {
                        (reason.effect.clone(), recovery_value(recovery))
                    }
                    Outcome::CoverageInsufficient { recovery, .. } => (
                        String::new(),
                        recovery.as_ref().map(recovery_value).unwrap_or(Value::Null),
                    ),
                    _ => (String::new(), Value::Null),
                },
                Err(error) => (error.to_string(), Value::Null),
            };
            let class = support::class(&result);
            let expected_exit = case["exit"].as_i64().unwrap_or(
                if ["D", "F", "UR", "UO"].contains(&case["class"].as_str().unwrap()) {
                    2
                } else {
                    0
                },
            );
            let mut problems = Vec::new();
            if class != case["class"] {
                problems.push("class");
            }
            if i64::from(wire.exit) != expected_exit {
                problems.push("wire");
            }
            if let Some(gaps) = case["gaps"].as_array() {
                for gap in gaps {
                    if !coverage["gaps"]
                        .as_array()
                        .is_some_and(|values| values.contains(gap))
                    {
                        problems.push("gap");
                    }
                }
            }
            if let Some(expected) = case["reason"].as_str() {
                if !reason.contains(expected) {
                    problems.push("reason");
                }
            }
            if case["owner_action"] == true && recovery["next_step"]["kind"] != "owner_action" {
                problems.push("owner action");
            }
            if case["no_oracle"] == true && recovery.get("objective").is_some() {
                problems.push("oracle");
            }
            if case["not_qualifier"] == true
                && recovery.to_string().contains("Zsh executable qualifier")
            {
                problems.push("inert qualifier");
            }
            if let Some(stdout) = case["stdout"].as_str() {
                if wire.stdout != stdout {
                    problems.push("stdout");
                }
            }
            if class == "UR"
                && coverage["gaps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|gap| wire.stderr.contains(gap.as_str().unwrap()))
            {
                problems.push("Debug gap in wire");
            }
            println!(
                "{}",
                json!({"id":case["id"],"finding":case["finding"],"arm":format!("{arm:?}"),"problems":problems,"class":class,"coverage":coverage,"reason":reason,"recovery":recovery})
            );
            if !problems.is_empty() {
                failures.push(format!("{} {arm:?} {problems:?}", case["id"]));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn review_shell() {
    check_group("shell");
}

#[test]
fn review_program() {
    check_group("program");
}

#[test]
fn review_identity() {
    check_group("identity");
}

#[test]
fn review_metadata() {
    check_group("metadata");
}

#[test]
fn review_wire() {
    check_group("wire");
}
