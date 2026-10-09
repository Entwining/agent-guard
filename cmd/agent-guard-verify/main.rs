#![deny(clippy::unwrap_used, clippy::expect_used)]
#![forbid(unsafe_code)]

#[path = "../public_tools.rs"]
mod public_tools;
mod verify;

fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(verify::main(&std::env::args().skip(1).collect::<Vec<_>>()) as u8)
}
