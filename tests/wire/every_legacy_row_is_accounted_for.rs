use crate::differential;

#[test]
fn every_legacy_row_is_accounted_for() {
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        let report = differential::report(arm);
        for row in &report {
            for observation in row["observations"].as_array().into_iter().flatten() {
                assert!(
                    !observation["go_deny_rust_permit"].as_bool().unwrap()
                        || observation["category"] == "intended_change",
                    "{} {}: unruled contract Go-deny/Rust-permit ({})",
                    row["id"],
                    observation["consumer"],
                    observation["category"]
                );
                assert_eq!(
                    observation["exit"],
                    if matches!(
                        observation["actual"].as_str().unwrap(),
                        "D" | "UR" | "UO" | "F"
                    ) {
                        2
                    } else {
                        0
                    },
                    "{} public exit",
                    row["id"]
                );
                let advice = observation["actual"] == "A" && observation["consumer"] == "claude";
                assert_eq!(
                    !observation["stdout"].as_str().unwrap().is_empty(),
                    advice,
                    "{} unexpected or absent advice",
                    row["id"]
                );
                if matches!(observation["actual"].as_str().unwrap(), "N" | "UC" | "A") {
                    assert!(
                        observation["stderr"].as_str().unwrap().is_empty(),
                        "{} unexpected denial",
                        row["id"]
                    );
                }
                if observation["go_rule"] == "AppData" && observation["actual"] == "D" {
                    assert_eq!(
                        observation["rust_rule"], "AppData",
                        "{}: a protected App Data target needs its rule's alternative",
                        row["id"]
                    );
                }
                if matches!(
                    row["id"].as_str(),
                    Some("appdata[15]" | "appdata[79]" | "search[86]")
                ) {
                    assert_eq!(
                        observation["rust_rule"], "Broad",
                        "{}: lexical Library-root scans need the project-scope alternative",
                        row["id"]
                    );
                }
                assert!(
                    !observation["stderr"]
                        .as_str()
                        .unwrap()
                        .contains("recovery: {"),
                    "{} {}: recovery belongs to diagnostics",
                    row["id"],
                    observation["consumer"]
                );
                if observation["same_denial_rule"] == true {
                    assert_eq!(
                        observation["consumer_text_match"], true,
                        "{} {} {}: consumer reason differs from Go",
                        row["id"], observation["consumer"], observation["go_rule"]
                    );
                }
            }
        }
        let defects: Vec<_> = report
            .iter()
            .flat_map(|r| {
                r["observations"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(move |o| (r, o))
            })
            .filter(|(_, o)| o["category"] == "Rust_defect")
            .map(|(r, o)| {
                format!(
                    "{} {} {} -> {} reason={} advice={}",
                    r["id"],
                    o["consumer"],
                    o["expected"],
                    o["actual"],
                    o["reason_match"],
                    o["advice_match"]
                )
            })
            .collect();
        assert!(
            defects.is_empty(),
            "{arm:?} slice defects:\n{}",
            defects.join("\n")
        );
    }
}
