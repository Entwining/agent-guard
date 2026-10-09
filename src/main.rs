#![cfg_attr(not(test), deny(clippy::unwrap_used))]

use std::process::ExitCode;

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    ExitCode::from(agent_guard_rust::entry::main(&args) as u8)
}
