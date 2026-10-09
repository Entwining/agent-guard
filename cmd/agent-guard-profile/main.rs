#![deny(clippy::unwrap_used, clippy::expect_used)]
#![forbid(unsafe_code)]

mod profile;
#[path = "../public_tools.rs"]
mod public_tools;

fn main() -> std::process::ExitCode {
    match profile::generate(
        &std::env::args().skip(1).collect::<Vec<_>>(),
        &mut std::io::stdout(),
    ) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("FAIL: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
