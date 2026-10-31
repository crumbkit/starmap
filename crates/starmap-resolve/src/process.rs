//! The one module of starmap-resolve that starts a process (PROC-1). Its
//! one operation is [`op_read`], which reads one secret with the 1Password
//! CLI.

use std::io::{self, Read};
use std::path::Path;
use std::process::{self, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Instant;

use zeroize::Zeroizing;

#[cfg(test)]
mod tests;

/// The most bytes of standard output that `op read` may print. A longer
/// output is refused, because no secret that starmap reads is that long.
pub(crate) const OUTPUT_LIMIT: usize = 64 * 1024;

/// The most bytes of standard error that the operation keeps.
const ERROR_LIMIT: usize = 16 * 1024;

/// How a run of `op read` ended.
pub(crate) enum OpRead {
    /// `op` exited with success and printed these bytes.
    Printed(Zeroizing<Vec<u8>>),
    /// `op` exited with `status` and printed `message` on standard error.
    Failed { status: ExitStatus, message: String },
    /// `op` printed more than [`OUTPUT_LIMIT`] bytes, and the operation
    /// stopped it.
    TooLong,
    /// The deadline passed before `op` closed its output, and the
    /// operation stopped it.
    Deadline,
}

/// What a reader thread sends when its stream closes or reaches its limit.
enum Stream {
    Output(Zeroizing<Vec<u8>>),
    Error(Vec<u8>),
}

/// Runs `op read` for `reference` and returns how it ended. It states the
/// five items of PROC-2.
///
/// - Program: `program`, the path the host passed to
///   [`crate::OnePassword::new`]. The host chooses the program and trusts
///   whoever installed it. A bare name such as `op` is found on the `PATH`
///   of the process that runs starmap.
/// - Arguments: `read`, `--no-newline`, `--` and `reference`, each a
///   separate value. No shell parses them, and `--` ends the options, so
///   the reference is never read as one.
/// - Environment: the child inherits the whole environment of the process
///   that runs starmap, and the operation sets and removes no variable.
///   `op` finds its account, session and settings through variables such
///   as `HOME`, `XDG_CONFIG_HOME`, `OP_ACCOUNT` and
///   `OP_SERVICE_ACCOUNT_TOKEN`, and which of them it needs depends on how
///   the person signs in.
/// - Streams and deadline: standard input is empty, so `op` cannot wait for
///   typed input. Standard output is kept up to [`OUTPUT_LIMIT`] bytes, and
///   more is refused. Standard error is kept up to [`ERROR_LIMIT`] bytes.
///   `deadline` is the resolution's deadline, and it also covers any
///   1Password prompt that `op` shows.
/// - End: the operation stops and reaps the child. On the deadline or on
///   output over the limit it kills `op` and waits for it. After both
///   streams close it reads the exit status, and it kills a child that
///   still runs. The kill stops `op` only, and a process that `op` started
///   survives it (PROC-3). A descendant that inherits the pipes makes a
///   successful run wait until the deadline, and keeps the reader threads
///   alive after a kill until it closes them. `op read` writes no file, so
///   a stopped run leaves nothing behind.
///
/// # Errors
///
/// Returns the error of starting `op`, such as [`io::ErrorKind::NotFound`]
/// for a program that does not exist, of starting a reader thread, or of
/// waiting for it. A reader thread that cannot start stops `op` first.
pub(crate) fn op_read(program: &Path, reference: &str, deadline: Instant) -> io::Result<OpRead> {
    #[expect(
        clippy::disallowed_methods,
        reason = "PROC-1: op_read is the operation that starts op"
    )]
    let mut child = command(program, reference).spawn()?;
    let (sender, receiver) = mpsc::channel();
    let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
    let started = stdout
        .map_or(Ok(()), |pipe| read_output(pipe, sender.clone()))
        .and_then(|()| stderr.map_or(Ok(()), |pipe| read_error(pipe, sender)));
    if let Err(error) = started {
        // The thread's error is the one reported, so a failure to stop the
        // child adds nothing.
        let _ = stop(&mut child);
        return Err(error);
    }
    let mut output = Zeroizing::new(Vec::new());
    let mut error = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(left) {
            Ok(Stream::Output(bytes)) if bytes.len() > OUTPUT_LIMIT => {
                stop(&mut child)?;
                return Ok(OpRead::TooLong);
            }
            Ok(Stream::Output(bytes)) => output = bytes,
            Ok(Stream::Error(bytes)) => error = bytes,
            Err(RecvTimeoutError::Timeout) => {
                stop(&mut child)?;
                return Ok(OpRead::Deadline);
            }
            // Both readers have sent their stream.
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = match child.try_wait()? {
        Some(status) => status,
        None => stop(&mut child)?,
    };
    if status.success() {
        Ok(OpRead::Printed(output))
    } else {
        Ok(OpRead::Failed {
            status,
            message: String::from_utf8_lossy(&error).into_owned(),
        })
    }
}

/// Returns the command of `op read` for `reference`, which [`op_read`]
/// starts.
fn command(program: &Path, reference: &str) -> Command {
    #[expect(
        clippy::disallowed_methods,
        reason = "PROC-1: op_read is the operation that starts op"
    )]
    let mut command = Command::new(program);
    command
        .args(["read", "--no-newline", "--", reference])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// Kills `child`, waits for it, and returns its exit status. It waits even
/// when the kill fails, such as for a child that has already exited, so the
/// child is always reaped.
#[expect(
    clippy::disallowed_types,
    reason = "PROC-1: op_read owns its child and stops it"
)]
fn stop(child: &mut process::Child) -> io::Result<ExitStatus> {
    let killed = child.kill();
    let status = child.wait()?;
    killed.map(|()| status)
}

/// Reads standard output on a new thread, up to one byte over
/// [`OUTPUT_LIMIT`], and sends it when the pipe closes or the limit is
/// passed. The buffer holds the limit from the start, so reading never
/// moves the secret and leaves a copy behind.
fn read_output(pipe: impl Read + Send + 'static, sender: Sender<Stream>) -> io::Result<()> {
    thread::Builder::new().spawn(move || {
        let mut bytes = Zeroizing::new(Vec::with_capacity(OUTPUT_LIMIT + 1));
        // A read that fails keeps the bytes read until then, and the exit
        // status reports the failure.
        let _ = pipe.take(limit(OUTPUT_LIMIT + 1)).read_to_end(&mut bytes);
        // The operation stops listening when it stops the child, so a
        // failed send is not a problem.
        let _ = sender.send(Stream::Output(bytes));
    })?;
    Ok(())
}

/// Reads standard error on a new thread, up to [`ERROR_LIMIT`] bytes, and
/// sends what it read when the pipe closes or the limit is reached.
fn read_error(pipe: impl Read + Send + 'static, sender: Sender<Stream>) -> io::Result<()> {
    thread::Builder::new().spawn(move || {
        let mut bytes = Vec::new();
        // A read that fails keeps the bytes read until then.
        let _ = pipe.take(limit(ERROR_LIMIT)).read_to_end(&mut bytes);
        let _ = sender.send(Stream::Error(bytes));
    })?;
    Ok(())
}

/// Returns `bytes` as the limit of [`Read::take`].
fn limit(bytes: usize) -> u64 {
    u64::try_from(bytes).unwrap_or(u64::MAX)
}
