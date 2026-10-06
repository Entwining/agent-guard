#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    if arguments
        .next()
        .is_some_and(|argument| argument == "--version")
        && arguments.next().is_none()
    {
        println!(
            "agent-guard-rust-slice {}",
            include_str!("../VERSION").trim()
        );
        return ExitCode::SUCCESS;
    }
    eprintln!("agent-guard-rust-slice only supports --version; no check was performed.");
    ExitCode::FAILURE
}
