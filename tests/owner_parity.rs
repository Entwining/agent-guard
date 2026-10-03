#[path = "support/differential.rs"]
mod differential;
mod support;
use agent_guard_rust::shell::Arm;

fn required(ids: &[&str]) {
    for arm in [Arm::Brush, Arm::TreeSitter] {
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

#[test]
fn listing_option_values_and_children() {
    required(&[
        "appdata[54]",
        "appdata[55]",
        "appdata[56]",
        "appdata[89]",
        "appdata[90]",
        "appdata[119]",
        "appdata[127]",
        "programs[18]",
        "programs[19]",
        "programs[66]",
        "readers[17]",
        "readers[18]",
        "readers[19]",
        "readers[22]",
        "readers[23]",
        "readers[104]",
        "search[95]",
        "search[96]",
        "search[97]",
        "search[98]",
        "search[100]",
    ]);
}

#[test]
fn readlink_parents_and_firmlink_spelling() {
    required(&[
        "appdata[96]",
        "appdata[101]",
        "appdata[108]",
        "appdata[125]",
        "credentials[173]",
        "cwd[2]",
        "cwd[4]",
        "cwd[63]",
        "cwd[64]",
        "cwd[65]",
    ]);
}

#[test]
fn grep_ag_and_versioned_interpreter_roles() {
    required(&[
        "appdata[19]",
        "credentials[290]",
        "readers[15]",
        "search[34]",
        "search[35]",
        "interpreters[32]",
        "options[0]",
    ]);
}

#[test]
fn interpreter_environment_options_use_paths_without_extracting_contents() {
    required(&[
        "credentials[141]",
        "options[1]",
        "programs[49]",
        "programs[51]",
    ]);
}
