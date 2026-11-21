pub use crate::differential;

pub fn required(ids: &[&str]) {
    let report = differential::selected_report(ids);
    for id in ids {
        let row = report.iter().find(|row| row["id"] == *id).unwrap();
        assert!(row["scope"].is_null(), "{id} is modelled");
        for observation in row["observations"].as_array().unwrap() {
            assert_ne!(
                observation["category"], "Rust_defect",
                "{id}: {observation}"
            );
        }
    }
}
