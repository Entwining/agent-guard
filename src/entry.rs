//! Executable boundary for the offline Rust trial; no hook registers this runner.

use crate::{
    Context, Event,
    adapters::{self, Consumer},
    filesystem::DiskProbe,
};
use std::{
    cell::Cell,
    io::{self, Read, Write},
    os::fd::AsFd,
    os::unix::process::ExitStatusExt,
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

pub const CHECKER_TIMEOUT: Duration = Duration::from_millis(2500);
pub const SUPERVISOR_TIMEOUT: Duration = Duration::from_millis(2800);

const USAGE: &str = "usage: agent-guard --runtime claude|codex|pi < event.json\n";

fn options(args: &[String]) -> Option<(Consumer, String)> {
    let mut runtime = "";
    let mut cwd = "";
    let mut index = 0;
    while index < args.len() {
        let (key, value) = if let Some(pair) = args[index].split_once('=') {
            pair
        } else {
            let key = args[index].as_str();
            index += 1;
            let value = args.get(index)?.as_str();
            if value != "-" && value.starts_with('-') {
                return None;
            }
            (key, value)
        };
        match key {
            "--runtime" => runtime = value,
            "--cwd" => cwd = value,
            _ => return None,
        }
        index += 1;
    }
    let consumer = match runtime {
        "claude" => Consumer::Claude,
        "codex" => Consumer::Codex,
        "pi" => Consumer::Pi,
        _ => return None,
    };
    (!cwd.is_empty()).then(|| (consumer, cwd.to_owned()))
}

pub fn check(
    args: &[String],
    input: &mut dyn Read,
    output: &mut dyn Write,
    error: &mut dyn Write,
) -> i32 {
    let deadline = Instant::now() + CHECKER_TIMEOUT;
    let Some((consumer, cwd)) = options(args) else {
        let _ = error.write_all(USAGE.as_bytes());
        return 2;
    };
    let mut bytes = Vec::new();
    let result = (|| -> io::Result<_> {
        input.read_to_end(&mut bytes)?;
        let home = std::fs::canonicalize(std::env::var_os("HOME").unwrap_or_default())?;
        let home = home
            .to_str()
            .ok_or_else(|| io::Error::other("HOME is not UTF-8"))?;
        let context = Context {
            consumer,
            home: home.to_owned(),
            user: std::env::var("USER").ok(),
            cwd,
            zsh_executor: consumer != Consumer::Pi,
            require_execution_owner: false,
            shell_observation_entries: Cell::new(0),
        };
        let result = crate::policy::evaluate_native(
            Event {
                bytes: &bytes,
                context: &context,
                probe: &mut DiskProbe,
            },
            deadline,
        );
        let wire = adapters::render_native(consumer, &result);
        output.write_all(wire.stdout.as_bytes())?;
        error.write_all(wire.stderr.as_bytes())?;
        Ok(if result.is_err() { 1 } else { wire.exit })
    })();
    match result {
        Ok(status) => status,
        Err(fault) => {
            let _ = writeln!(error, "{fault}");
            1
        }
    }
}

pub fn checker_status(status: i32) -> i32 {
    if status == 2 { 3 } else { status }
}

pub fn status_code(status: ExitStatus) -> i32 {
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(1))
}

pub fn supervise(command: &mut Command) -> io::Result<i32> {
    let deadline = Instant::now() + SUPERVISOR_TIMEOUT;
    let mut child = command.spawn()?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status_code(status)),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(2)),
            result => {
                // Even a polling or kill error must finish ownership of the child.
                let killed = child.kill();
                let waited = child.wait();
                result?;
                killed?;
                return waited.map(status_code);
            }
        }
    }
}

pub fn run(args: &[String]) -> io::Result<i32> {
    let mut child = Command::new(std::env::current_exe()?);
    // Both descriptors point at the same destination, as in the Go runner.
    // No intermediary pipe can stall while the parent waits for the checker.
    let stdout = io::stdout().as_fd().try_clone_to_owned()?;
    child
        .arg("--supervised-checker")
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::from(stdout));
    supervise(&mut child)
}

pub fn main(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("--version") => {
            if args.len() != 1 {
                eprint!("{USAGE}");
                return 2;
            }
            if writeln!(
                io::stdout(),
                "agent-guard {}",
                include_str!("../VERSION").trim()
            )
            .is_err()
            {
                return 1;
            }
            0
        }
        Some("--checker" | "--supervised-checker") => {
            let status = check(
                &args[1..],
                &mut io::stdin(),
                &mut io::stdout(),
                &mut io::stderr(),
            );
            if args[0] == "--supervised-checker" {
                checker_status(status)
            } else {
                status
            }
        }
        _ => match run(args) {
            Ok(status) => status,
            Err(error) => {
                eprintln!("{error}");
                1
            }
        },
    }
}
