use std::path::PathBuf;

fn profile_directory() -> PathBuf {
    // Cargo places example tools in <profile>/examples; fixtures stay with those artifacts.
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned()
}

impl super::support::Fixture {
    pub fn new() -> Self {
        Self::new_at(&profile_directory().join("tmp"))
    }

    pub fn worker_binary() -> PathBuf {
        profile_directory().join("agent-guard-rust-fixture-worker")
    }
}
