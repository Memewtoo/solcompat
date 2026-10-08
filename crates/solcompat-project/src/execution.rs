//! Opt-in child execution with bounded retained output and a wall-clock deadline.
use anyhow::{Context, Result};
use std::{
    collections::VecDeque,
    io::Read,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub(crate) struct Captured {
    pub(crate) success: bool,
    pub(crate) timed_out: bool,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) truncated: bool,
}
struct Stream {
    bytes: Mutex<VecDeque<u8>>,
    done: AtomicBool,
    truncated: AtomicBool,
    failed: AtomicBool,
}
fn drain(reader: impl Read + Send + 'static, limit: usize) -> Arc<Stream> {
    let stream = Arc::new(Stream {
        bytes: Mutex::new(VecDeque::new()),
        done: AtomicBool::new(false),
        truncated: AtomicBool::new(false),
        failed: AtomicBool::new(false),
    });
    let shared = stream.clone();
    thread::spawn(move || {
        let mut reader = reader;
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    let mut bytes = shared.bytes.lock().unwrap();
                    bytes.extend(&buffer[..n]);
                    while bytes.len() > limit {
                        bytes.pop_front();
                        shared.truncated.store(true, Ordering::Relaxed);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    shared.failed.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }
        shared.done.store(true, Ordering::Release);
    });
    stream
}
fn text(stream: &Stream) -> String {
    let bytes: Vec<_> = stream.bytes.lock().unwrap().iter().copied().collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn terminate(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Unix builds get a separate process group so timeout cleanup includes descendants.
/// An escaped descendant cannot make the caller wait on a reader-thread join.
pub(crate) fn run(command: &mut Command, timeout: Duration, limit: usize) -> Result<Captured> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().context("cannot start child process")?;
    let out = drain(child.stdout.take().unwrap(), limit);
    let err = drain(child.stderr.take().unwrap(), limit);
    let started = Instant::now();
    let mut status = None;
    let mut timed_out = false;
    loop {
        if status.is_none() {
            match child.try_wait() {
                Ok(value) => status = value,
                Err(error) => {
                    terminate(&mut child);
                    return Err(error.into());
                }
            }
        }
        if status.is_some() && out.done.load(Ordering::Acquire) && err.done.load(Ordering::Acquire)
        {
            break;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            terminate(&mut child);
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(Captured {
        success: !timed_out
            && status.is_some_and(|s| s.success())
            && !out.failed.load(Ordering::Relaxed)
            && !err.failed.load(Ordering::Relaxed),
        timed_out,
        stdout: text(&out),
        stderr: text(&err),
        truncated: out.truncated.load(Ordering::Relaxed) || err.truncated.load(Ordering::Relaxed),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn output_is_drained_without_deadlock_and_retention_is_bounded() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c","i=0; while [ $i -lt 20000 ]; do echo 12345678901234567890; echo stderr >&2; i=$((i+1)); done"]);
        let output = run(&mut command, Duration::from_secs(5), 1024).unwrap();
        assert!(output.success);
        assert!(output.truncated);
        assert!(output.stdout.len() <= 1024);
        assert!(output.stderr.len() <= 1024);
    }
    #[test]
    fn timeout_includes_descendants_holding_output_pipes() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 30 & exit 0"]);
        let start = Instant::now();
        let output = run(&mut command, Duration::from_millis(100), 1024).unwrap();
        assert!(output.timed_out);
        assert!(!output.success);
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
