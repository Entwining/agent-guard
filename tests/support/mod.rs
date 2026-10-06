mod harness;
pub use harness::*;

impl Fixture {
    pub fn new() -> Self {
        Self::new_at(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")))
    }

    pub fn worker_binary() -> std::path::PathBuf {
        env!("CARGO_BIN_EXE_agent-guard-rust-fixture-worker").into()
    }
}
