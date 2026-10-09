use std::{
    io::{self, Read, Write},
    os::{
        fd::AsFd,
        unix::process::{CommandExt, ExitStatusExt},
    },
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use nix::{
    fcntl::{FcntlArg, OFlag, fcntl},
    sys::signal::{Signal, killpg},
    unistd::Pid,
};

pub fn io_message(error: &io::Error) -> String {
    let text = error.to_string();
    text.split(" (os error ")
        .next()
        .unwrap_or(&text)
        .to_lowercase()
}

pub fn path_error(operation: &str, path: &Path, error: &io::Error) -> String {
    format!("{operation} {}: {}", path.display(), io_message(error))
}

pub struct ResultOutput {
    pub exit: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub signal: Option<i32>,
}

fn drain(pipe: &mut impl Read, output: &mut Vec<u8>) -> io::Result<bool> {
    let mut buffer = [0; 8192];
    // A continuously writing child must not postpone the deadline check.
    for _ in 0..8 {
        match pipe.read(&mut buffer) {
            Ok(0) => return Ok(true),
            Ok(count) => output.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

fn nonblocking(pipe: &impl AsFd) -> io::Result<()> {
    let flags = OFlag::from_bits_truncate(fcntl(pipe, FcntlArg::F_GETFL)?);
    fcntl(pipe, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
    Ok(())
}

pub fn execute(
    command: &mut Command,
    body: &[u8],
    timeout: Duration,
    pipe_delay: Duration,
    interrupted: &AtomicUsize,
    merged: bool,
) -> Result<ResultOutput, String> {
    if interrupted.load(Ordering::Relaxed) != 0 {
        return Err("context canceled".into());
    }
    let path = command.get_program().to_string_lossy().into_owned();
    let deadline = Instant::now() + timeout;
    let (mut child, mut out, mut err) = spawn_with_pipes(command, merged, &path)?;
    let pid = Pid::from_raw(child.id() as i32);
    let stop = || kill_child_group(pid);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut timed_out = false;
    let mut cleanup = None;
    let operation = (|| -> io::Result<_> {
        let input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("missing child stdin"))?;
        nonblocking(&input)?;
        let mut written = 0;
        nonblocking(&out)?;
        nonblocking(&err)?;
        let mut input = Some(input);
        let mut status = None;
        let mut exited_at = None;
        std::thread::scope(|scope| {
            let (completed, completion) = mpsc::sync_channel(1);
            // Reap the direct child independently of group cancellation;
            // postponing wait until after killpg changes completion ordering.
            let child = &mut child;
            let waiter = std::thread::Builder::new().spawn_scoped(scope, move || {
                let _ = completed.send(child.wait());
            })?;
            let operation = (|| {
                loop {
                    if let Some(pipe) = input.as_mut() {
                        match pipe.write(&body[written..]) {
                            Ok(count) => written += count,
                            Err(error)
                                if matches!(
                                    error.kind(),
                                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                                ) => {}
                            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => input = None,
                            Err(error) => return Err(error),
                        }
                        if written == body.len() {
                            input = None;
                        }
                    }
                    let out_done = drain(&mut out, &mut stdout)?;
                    let err_done = drain(&mut err, &mut stderr)?;
                    if interrupted.load(Ordering::Relaxed) != 0 || Instant::now() >= deadline {
                        timed_out = interrupted.load(Ordering::Relaxed) == 0;
                        stop()?;
                        let status = match status {
                            Some(status) => status,
                            None => completion.recv().map_err(io::Error::other)??,
                        };
                        drain_after_stop(&mut out, &mut err, &mut stdout, &mut stderr, pipe_delay)?;
                        return Ok(status);
                    }
                    if status.is_none() {
                        status = match completion.try_recv() {
                            Ok(result) => Some(result?),
                            Err(mpsc::TryRecvError::Empty) => None,
                            Err(error) => return Err(io::Error::other(error)),
                        };
                        if status.is_some() {
                            exited_at = Some(Instant::now());
                        }
                    }
                    if let Some(status) = status.filter(|_| out_done && err_done) {
                        return Ok(status);
                    }
                    if exited_at.is_some_and(|instant| instant.elapsed() >= pipe_delay) {
                        return Err(io::Error::other(
                            "exec: WaitDelay expired before I/O complete",
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            })();
            if operation.is_err() {
                cleanup = Some(stop());
            }
            waiter
                .join()
                .map_err(|_| io::Error::other("child waiter failed"))?;
            operation
        })
    })();
    // Completed I/O releases its read ends before final group cleanup.
    if operation.is_ok() {
        drop(out);
        drop(err);
    }
    // Cleanup and reaping run even when stdin, output collection, or polling fails.
    let cleanup = cleanup.unwrap_or_else(stop);
    let reaped = child.wait();
    cleanup.map_err(|error| io_message(&error))?;
    reaped.map_err(|error| io_message(&error))?;
    completed_output(operation, &stdout, &stderr, timed_out)
}

fn spawn_with_pipes(
    command: &mut Command,
    merged: bool,
    path: &str,
) -> Result<(std::process::Child, io::PipeReader, io::PipeReader), String> {
    let (out, out_writer) = io::pipe().map_err(|error| io_message(&error))?;
    let (err, err_writer) = io::pipe().map_err(|error| io_message(&error))?;
    command.stderr(Stdio::from(err_writer));
    if merged {
        command.stderr(Stdio::from(
            out_writer.try_clone().map_err(|error| io_message(&error))?,
        ));
    }
    let spawned = command
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::from(out_writer))
        .spawn();
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let child = spawned.map_err(|error| format!("fork/exec {path}: {}", io_message(&error)))?;
    Ok((child, out, err))
}

fn completed_output(
    operation: io::Result<std::process::ExitStatus>,
    stdout: &[u8],
    stderr: &[u8],
    timed_out: bool,
) -> Result<ResultOutput, String> {
    let status = operation.map_err(|error| {
        if error.raw_os_error().is_none() {
            error.to_string()
        } else {
            io_message(&error)
        }
    })?;
    Ok(ResultOutput {
        exit: status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(stdout).into_owned(),
        stderr: String::from_utf8_lossy(stderr).into_owned(),
        timed_out,
        signal: status.signal(),
    })
}

fn drain_after_stop(
    out: &mut io::PipeReader,
    err: &mut io::PipeReader,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
    pipe_delay: Duration,
) -> io::Result<()> {
    let until = Instant::now() + pipe_delay;
    while !(drain(out, stdout)? && drain(err, stderr)?) {
        if Instant::now() >= until {
            return Err(io::Error::other(
                "exec: WaitDelay expired before I/O complete",
            ));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

fn kill_child_group(pid: Pid) -> io::Result<()> {
    match killpg(pid, Signal::SIGKILL) {
        // Darwin answers EPERM, not ESRCH, while the group's only members are
        // exited children the waiter has not reaped yet. Every member runs under
        // this tool's uid, so EPERM has no other cause unless a member gains
        // another identity, such as through a setuid program.
        Ok(()) | Err(nix::errno::Errno::ESRCH | nix::errno::Errno::EPERM) => Ok(()),
        Err(error) => Err(io::Error::from(error)),
    }
}
