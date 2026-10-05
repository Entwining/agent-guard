#[path = "support/batch6.rs"]
mod batch7;

#[test]
fn recognised_wrappers_preserve_effective_git_environment() {
    batch7::partition(
        "wrapper",
        include_str!("fixtures/rust-batch7-wrappers.json"),
    );
}

#[test]
fn privileged_wrapper_assignments_reach_the_child() {
    batch7::partition("sudo", include_str!("fixtures/rust-batch7-wrappers.json"));
}

#[test]
fn wrapper_program_names_and_environment_removal_stay_public() {
    batch7::partition(
        "control",
        include_str!("fixtures/rust-batch7-wrappers.json"),
    );
}
