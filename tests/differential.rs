#[path = "support/differential.rs"]
mod differential;
mod support;

#[test]
fn every_legacy_row_is_accounted_for() {
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        let report = differential::report(arm);
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
