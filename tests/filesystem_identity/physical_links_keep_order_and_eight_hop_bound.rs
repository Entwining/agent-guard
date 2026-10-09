use crate::support;
use agent_guard_rust::{
    filesystem::{self, Identity, Metadata, Probe},
    shell::Arm,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

struct Mock {
    links: BTreeMap<String, String>,
    calls: Vec<String>,
    fault: Option<String>,
}
impl Probe for Mock {
    fn read_link(&mut self, path: &Path) -> io::Result<Option<PathBuf>> {
        let path = path.to_str().unwrap();
        self.calls.push(path.to_owned());
        assert!(
            filesystem::lexical_literal(path, "/h").is_none(),
            "protected probe: {path}"
        );
        if self.fault.as_deref() == Some(path) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        Ok(self.links.get(path).map(PathBuf::from))
    }
    fn stat(&mut self, _: &Path) -> io::Result<Option<Metadata>> {
        panic!("App Data resolution must not stat");
    }
}

#[test]
fn physical_links_keep_order_and_eight_hop_bound() {
    let packet: Value =
        serde_json::from_str(include_str!("../fixtures/rust-m2-filesystem.json")).unwrap();
    let rows: Vec<_> = packet["paths"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r.get("catalog").is_none())
        .collect();
    assert!(!rows.is_empty(), "missing physical link partition");
    for row in rows {
        let mut probe = Mock {
            links: row
                .get("links")
                .map(|v| serde_json::from_value(v.clone()).unwrap())
                .unwrap_or_default(),
            calls: Vec::new(),
            fault: row["fault"].as_str().map(str::to_owned),
        };
        let result = filesystem::identify_target(
            row["path"].as_str().unwrap(),
            "/project",
            row["home"].as_str().unwrap_or("/h"),
            false,
            row["patterned"].as_bool().unwrap_or(false),
            agent_guard_rust::record::Effect::Use,
            &mut probe,
        );
        if let Some(kind) = row["protected"].as_str() {
            assert!(
                matches!(result, Ok(Identity::Protected(p)) if format!("{p:?}") == kind),
                "{}: {result:?}",
                row["id"]
            );
        } else if let Some(path) = row["public"].as_str() {
            assert_eq!(
                result.unwrap(),
                Identity::Public(path.into()),
                "{}",
                row["id"]
            );
        } else if row["bound"] == true {
            assert_eq!(result.unwrap(), Identity::Bound, "{}", row["id"]);
        } else {
            assert!(result.is_err(), "{}: {result:?}", row["id"]);
            assert_eq!(
                format!("{:?}", result.unwrap_err().kind),
                row["error"].as_str().unwrap()
            );
        }
        if row["zero_probes"] == true {
            assert!(probe.calls.is_empty(), "{}", row["id"]);
        }
        if let Some(last) = row["last_probe"].as_str() {
            assert_eq!(probe.calls.last().unwrap(), last, "{}", row["id"]);
        }
        println!(
            "{}",
            serde_json::json!({"id":row["id"], "readlink":probe.calls, "stat":[]})
        );
    }
}

#[test]
fn injected_host_catalog_reaches_all_consumer_boundaries() {
    use agent_guard_rust::{Context, Event, adapters::Consumer, evaluate_with_catalog};
    let packet: Value =
        serde_json::from_str(include_str!("../fixtures/rust-m2-filesystem.json")).unwrap();
    let rows: Vec<_> = packet["paths"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r.get("catalog").is_some())
        .collect();
    assert!(!rows.is_empty(), "missing host catalog partition");
    for row in rows {
        for consumer in [Consumer::Claude, Consumer::Codex, Consumer::Pi] {
            let context = Context {
                consumer,
                home: "/h".into(),
                user: None,
                cwd: row["cwd"].as_str().unwrap_or("/project").into(),
                zsh_executor: consumer != Consumer::Pi,
                require_execution_owner: false,
                shell_observation_entries: std::cell::Cell::new(0),
            };
            let input = if row["operation"] == "shell" {
                serde_json::json!({"tool_name":if consumer == Consumer::Pi {"bash"} else {"Bash"},"tool_input":{"command":row["source"]}})
            } else if consumer == Consumer::Pi {
                serde_json::json!({"toolName":"read","input":{"path":row["path"]}})
            } else if consumer == Consumer::Codex {
                serde_json::json!({"name":"Read","arguments":{"file_path":row["path"]}})
            } else {
                serde_json::json!({"tool_name":"Read","tool_input":{"file_path":row["path"]}})
            };
            let mut probe = Mock {
                links: row
                    .get("links")
                    .map(|v| serde_json::from_value(v.clone()).unwrap())
                    .unwrap_or_default(),
                calls: Vec::new(),
                fault: None,
            };
            let bytes = serde_json::to_vec(&input).unwrap();
            let result = evaluate_with_catalog(
                Event {
                    bytes: &bytes,
                    context: &context,
                    probe: &mut probe,
                },
                Arm::Brush,
                filesystem::FirmlinkTable::from_text(row["catalog"].as_str().unwrap()),
            );
            // Claude Code and Pi remove `..` from a tool path before any link
            // or firmlink is followed, so they open `tool_opens` instead.
            let lexical_tool = row["operation"] != "shell" && consumer != Consumer::Codex;
            assert_eq!(
                support::class(&result),
                if row["protected"].is_string() && !(lexical_tool && row["tool_opens"].is_string())
                {
                    "D"
                } else {
                    "N"
                },
                "{}: {consumer:?}: {result:?}",
                row["id"]
            );
            println!(
                "{}",
                serde_json::json!({"id":row["id"],"consumer":format!("{consumer:?}"),"readlink":probe.calls,"stat":[]})
            );
        }
    }
}

#[test]
fn directory_targets_keep_name_glob_and_enter_contracts() {
    use agent_guard_rust::{Event, adapters, evaluate_with_arm};
    let packet: Value =
        serde_json::from_str(include_str!("../fixtures/rust-m2-filesystem.json")).unwrap();
    let fixture = support::Fixture::new();
    let rows = packet["rows"].as_array().unwrap();
    assert!(!rows.is_empty(), "missing directory target partition");
    for row in rows {
        for consumer in ["claude", "codex", "pi"] {
            let context =
                fixture.context(&serde_json::json!({"consumer":consumer,"cwd":fixture.project}));
            let source = row["source"]
                .as_str()
                .unwrap()
                .replace("/h/", &format!("{}/", fixture.home));
            let event = serde_json::to_vec(&serde_json::json!({"tool_name": if consumer == "pi" {"bash"} else {"Bash"}, "tool_input":{"command": source}})).unwrap();
            let mut probe = support::RecordingProbe::literal_for_quoted_paths(&fixture);
            let result = evaluate_with_arm(
                Event {
                    bytes: &event,
                    context: &context,
                    probe: &mut probe,
                },
                Arm::Brush,
            );
            assert_eq!(
                support::class(&result),
                row["expected"],
                "{}: {consumer}: {result:?}",
                row["id"]
            );
            let wire = adapters::render(context.consumer, &result);
            assert_eq!(
                wire.exit,
                if row["expected"] == "D" { 2 } else { 0 },
                "{}",
                row["id"]
            );
            assert!(wire.stdout.is_empty());
            if row["expected"] == "D" {
                let agent_guard_rust::Outcome::ProtectedDenial { reason, .. } =
                    &result.as_ref().unwrap().outcome
                else {
                    panic!("missing protected denial")
                };
                assert!(wire.stderr.contains(reason.rule.message()));
                assert!(
                    reason
                        .effect
                        .contains(row["reason"].as_str().unwrap_or("App Data")),
                    "{}: {wire:?}",
                    row["id"]
                );
            }
        }
    }
}
