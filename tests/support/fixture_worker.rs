#![forbid(unsafe_code)]

use std::{
    fs,
    io::{self, Write},
    time::Duration,
};

fn main() -> io::Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().is_some_and(|arg| arg == "runner") {
        std::process::exit(agent_guard_rust::entry::run(&arguments[1..])?);
    }
    if arguments
        .first()
        .is_some_and(|arg| arg == "--supervised-checker")
    {
        return fault_checker(&arguments[1..]);
    }
    let mut args = arguments.into_iter();
    let mode = args
        .next()
        .ok_or_else(|| io::Error::other("missing fixture mode"))?;
    let receipt = args
        .next()
        .ok_or_else(|| io::Error::other("missing fixture receipt path"))?;
    fs::write(receipt, format!("ready:{}\n", std::process::id()))?;
    if mode == "bounded-stall" {
        std::thread::sleep(Duration::from_millis(4200));
        return Ok(());
    }
    if mode == "signal" {
        std::process::Command::new("/bin/kill")
            .args(["-TERM", &std::process::id().to_string()])
            .status()?;
        return Ok(());
    }
    io::stdout().write_all(b"fixture worker ready\n")?;
    if mode == "fault" {
        std::process::exit(71);
    }
    loop {
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn fault_checker(arguments: &[String]) -> io::Result<()> {
    let mode = arguments
        .first()
        .ok_or_else(|| io::Error::other("missing fault mode"))?;
    let receipt = arguments
        .get(1)
        .ok_or_else(|| io::Error::other("missing receipt"))?;
    fs::write(receipt, format!("ready:{}\n", std::process::id()))?;
    if mode == "large-reason" {
        io::stdout().write_all(&vec![b'O'; 2 * 1024 * 1024])?;
        io::stderr().write_all(&vec![b'E'; 2 * 1024 * 1024])?;
        std::process::exit(agent_guard_rust::entry::checker_status(2));
    }
    struct FaultInput {
        panic: bool,
        cursor: io::Cursor<&'static [u8]>,
        stalled: bool,
    }
    impl io::Read for FaultInput {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            assert!(!self.panic, "fixture checker panic");
            if !self.stalled {
                std::thread::sleep(Duration::from_millis(2600));
                self.stalled = true;
            }
            io::Read::read(&mut self.cursor, buffer)
        }
    }
    let home = std::env::var("HOME").map_err(io::Error::other)?;
    let args = vec!["--runtime".into(), "claude".into(), "--cwd".into(), home];
    let mut input = FaultInput {
        panic: mode == "check-panic",
        cursor: io::Cursor::new(br#"{"tool_name":"Bash","tool_input":{"command":"true"}}"#),
        stalled: false,
    };
    let started = std::time::Instant::now();
    let status =
        agent_guard_rust::entry::check(&args, &mut input, &mut io::stdout(), &mut io::stderr());
    fs::write(
        format!("{receipt}.elapsed"),
        started.elapsed().as_micros().to_string(),
    )?;
    std::process::exit(agent_guard_rust::entry::checker_status(status));
}
