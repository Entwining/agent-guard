pub use crate::differential;

pub fn required(ids: &[&str]) {
    for &arm in agent_guard_rust::shell::ACCEPTANCE_ARMS {
        let report = differential::selected_report(arm, ids);
        for id in ids {
            let row = report.iter().find(|row| row["id"] == *id).unwrap();
            assert!(row["scope"].is_null(), "{id} is modelled");
            for observation in row["observations"].as_array().unwrap() {
                assert_ne!(
                    observation["category"], "Rust_defect",
                    "{id} {arm:?}: {observation}"
                );
            }
        }
    }
}
