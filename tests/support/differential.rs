#![allow(dead_code)]
#![expect(
    clippy::disallowed_methods,
    reason = "This fixture manages synthetic link identities and verifies their metadata without reading contents."
)]
use crate::support::{Fixture, RecordingProbe, class, coverage};
use agent_guard_rust::{
    Context, Event,
    adapters::{Consumer, render},
    evaluate_with_arm,
    shell::Arm,
};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

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

#[path = "differential/classification.rs"]
mod classification;
use classification::{go_reason_rule, outside_slice, reason_partition};
#[path = "differential/observations.rs"]
mod observations;
use observations::observe_contract;

pub fn report(arm: Arm) -> Vec<Value> {
    report_rows(arm, None)
}

pub fn selected_report(arm: Arm, ids: &[&str]) -> Vec<Value> {
    report_rows(arm, Some(ids))
}

fn report_rows(arm: Arm, selected: Option<&[&str]>) -> Vec<Value> {
    let legacy: Vec<Value> = include_str!("../fixtures/contract.jsonl")
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert!(
        !legacy.is_empty(),
        "missing legacy operation contract partition"
    );
    let overlay: Vec<Value> = include_str!("../fixtures/rust-contract-classification.jsonl")
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let (legacy_ids, expected_ids) = contract_ids(&legacy, selected);
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
    let mut ids = BTreeSet::new();
    let mut report = Vec::new();
    let fixture = fixture();
    let scope_rows: Vec<Value> = include_str!("../fixtures/rust-d22-scope.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let scope: BTreeMap<String, Value> = scope_rows
        .iter()
        .map(|row| (row["id"].as_str().unwrap().to_owned(), row.clone()))
        .collect();
    assert_eq!(scope.len(), scope_rows.len(), "duplicate scope input");
    assert_eq!(scope.keys().cloned().collect::<BTreeSet<_>>(), legacy_ids);
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
            observations.push(observe_contract(
                &fixture, row, &item, &outside, arm, name, consumer,
            ));
        }
        report.push(json!({"id":id,"family":row["family"],"arm":format!("{arm:?}"),"verdict":item["verdict"],"rule_id":item["rule_id"],"scope":outside,"observations":observations}));
    }
    assert_eq!(
        ids, expected_ids,
        "every contract and link input must be evaluated"
    );
    assert_eq!(report.len(), ids.len());
    report
}

fn contract_ids(
    legacy: &[Value],
    selected: Option<&[&str]>,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let legacy_ids: BTreeSet<_> = legacy
        .iter()
        .map(|row| format!("{}[{}]", row["family"].as_str().unwrap(), row["index"]))
        .collect();
    assert_eq!(legacy_ids.len(), legacy.len(), "duplicate contract input");
    let setup: Value = serde_json::from_str(include_str!("../fixtures/filesystem.json")).unwrap();
    let expected_ids: BTreeSet<String> = legacy_ids
        .iter()
        .cloned()
        .chain(
            setup["links"]
                .as_array()
                .unwrap()
                .iter()
                .map(|link| link[0].as_str().unwrap().to_owned()),
        )
        .filter(|id| selected.is_none_or(|selected| selected.contains(&id.as_str())))
        .collect();
    if let Some(selected) = selected {
        assert!(!selected.is_empty(), "missing requested contract partition");
        assert_eq!(
            expected_ids,
            selected.iter().map(|id| (*id).to_owned()).collect(),
            "requested contract partition contains missing IDs"
        );
    } else {
        assert!(
            !expected_ids.is_empty(),
            "missing complete contract partition"
        );
    }
    (legacy_ids, expected_ids)
}
