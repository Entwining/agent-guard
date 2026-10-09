#![deny(clippy::unwrap_used, clippy::expect_used)]
#![forbid(unsafe_code)]

#[path = "../../tests/harness/mod.rs"]
pub mod harness;

#[expect(
    clippy::disallowed_methods,
    reason = "This lifecycle executable boundary terminates with the harness status after cleanup."
)]
fn main() {
    std::process::exit(harness::cli::main(false));
}
