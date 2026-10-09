use std::{
    fs::{self, File},
    io::{self, Write},
    os::fd::AsFd,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

fn wait_for(path: &Path, what: &str) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !path.exists() {
        if Instant::now() >= deadline {
            return Err(io::Error::other(format!("{what} did not arrive")));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

/// Issues this process's own identity over the receipt connection on stdin and
/// starts a descendant that keeps its copy of the connection until it is killed.
fn record_with_descendant(role: &str, root: &Path) -> io::Result<File> {
    let mut receipt = File::from(io::stdin().as_fd().try_clone_to_owned()?);
    writeln!(
        receipt,
        "{}",
        serde_json::json!({
            "role": role,
            "pid": std::process::id(),
            "ppid": nix::unistd::getppid().as_raw(),
            "pgid": nix::unistd::getpgrp().as_raw(),
        })
    )?;
    if role != "descendant" {
        Command::new(std::env::current_exe()?)
            .args(["entry-group", "descendant"])
            .arg(root)
            .spawn()?;
        wait_for(&root.join("ready"), "descendant readiness")?;
    }
    Ok(receipt)
}

/// Runs as `agent-guard-native` behind the shell entry, or as its checker.
pub fn run(args: &[String]) -> io::Result<i32> {
    let mode = args
        .first()
        .ok_or_else(|| io::Error::other("missing mode"))?;
    let root = Path::new(
        args.get(1)
            .ok_or_else(|| io::Error::other("missing root"))?,
    );
    let checker = |mode: &str| {
        let args = ["entry-group", mode, &root.to_string_lossy()].map(String::from);
        agent_guard_rust::entry::run_group(&args)
    };
    match mode.as_str() {
        "complete" => checker("checker"),
        "session-leader" => {
            nix::unistd::setsid()?;
            checker("permit")
        }
        "permit" => Ok(0),
        "checker" => {
            let _receipt = record_with_descendant("checker", root)?;
            wait_for(&root.join("observed"), "parent receipt collection")?;
            io::stdout().write_all(b"fixture complete\n")?;
            Ok(0)
        }
        "hang" => {
            agent_guard_rust::entry::lead_process_group()?;
            let _receipt = record_with_descendant("runner", root)?;
            std::thread::sleep(Duration::from_secs(8));
            Ok(0)
        }
        "descendant" => {
            let _receipt = record_with_descendant("descendant", root)?;
            fs::write(root.join("ready"), [])?;
            std::thread::sleep(Duration::from_secs(8));
            Ok(0)
        }
        _ => Err(io::Error::other("unknown entry-group mode")),
    }
}
