//! The one module of starmap-resolve that starts a process (PROC-1). Its
//! operations are [`op_read`], which reads one secret with the 1Password
//! CLI, [`op_inject`], which reads several with one run of it, and, for the
//! `agent` feature, `agent_start`, which starts starmap's agent.

#[cfg(feature = "agent")]
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{self, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Instant;

use zeroize::Zeroizing;

#[cfg(test)]
mod tests;

/// The most bytes of standard output that `op read` may print, and that
/// each reference of an `op inject` template adds to its limit. A longer
/// output is refused, because no secret that starmap reads is that long.
pub(crate) const OUTPUT_LIMIT: usize = 64 * 1024;

/// The most bytes of standard error that the operation keeps.
const ERROR_LIMIT: usize = 16 * 1024;

/// How a run of `op` ended.
pub(crate) enum OpRun {
    /// `op` could not be started, such as a program that does not exist,
    /// so nothing was asked of 1Password.
    NotStarted(io::Error),
    /// `op` exited with success and printed these bytes.
    Printed(Zeroizing<Vec<u8>>),
    /// `op` exited with `status` and printed `message` on standard error.
    Failed { status: ExitStatus, message: String },
    /// `op` printed more than its output limit, and the operation stopped
    /// it.
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
/// five items of PROC-2, and [`op_inject`] differs from it only where its
/// own list says.
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
///   After that the reader closes the pipe, so a further write by `op`
///   fails, and `op` exits by SIGPIPE.
///   `deadline` is the resolution's deadline, and it also covers any
///   1Password prompt that `op` shows.
/// - End: the operation stops and reaps the child. On the deadline or on
///   output over the limit it kills `op` and waits for it. After both
///   streams close it kills and waits for `op` too, so a child that closed
///   its streams and still runs cannot hold the operation past the
///   deadline. A kill sent to a child that has exited and is not yet reaped
///   leaves its exit status unchanged, so a normal exit keeps its status.
///   The kill stops `op` only, and a process that `op` started
///   survives it (PROC-3). A descendant that inherits the pipes makes a
///   successful run wait until the deadline, and keeps the reader threads
///   alive after a kill until it closes them. `op read` writes no file, so
///   a stopped run leaves nothing behind.
///
/// # Errors
///
/// Returns the error of starting a reader thread, or of waiting for `op`.
/// A reader thread that cannot start stops `op` first. A program that
/// cannot be started, such as one that does not exist, is
/// [`OpRun::NotStarted`].
pub(crate) fn op_read(program: &Path, reference: &str, deadline: Instant) -> io::Result<OpRun> {
    run(
        read_command(program, reference),
        None,
        OUTPUT_LIMIT,
        deadline,
    )
}

/// Runs `op inject` with `template` on its standard input and returns how
/// it ended. `op` prints the template with each `{{ op://… }}` reference
/// replaced by its value. It states the items of PROC-2 where they differ
/// from [`op_read`].
///
/// - Arguments: `inject` alone. `op` reads the template from standard
///   input and prints to standard output, so it writes no file.
/// - Streams: standard input holds `template`, which a writer thread
///   writes and then closes, so `op` cannot wait for typed input. The
///   template holds references and boundary markers, and no value.
///   Standard output is kept up to `references` times [`OUTPUT_LIMIT`]
///   plus the template's length, which fits a full value for every
///   reference. The buffer holds that limit from the start, and reserving
///   it does not touch the pages until `op` writes to them. A write to a
///   pipe that `op` has closed fails with an error only while the process
///   ignores SIGPIPE, as the Rust runtime sets it before `main`. A host
///   must not reset SIGPIPE to its default action, or `op` exiting
///   before it reads the whole template would end the host.
///
/// # Errors
///
/// Returns the errors of [`op_read`], and the error of starting the writer
/// thread, which stops `op` first.
pub(crate) fn op_inject(
    program: &Path,
    template: String,
    references: usize,
    deadline: Instant,
) -> io::Result<OpRun> {
    let limit = OUTPUT_LIMIT
        .saturating_mul(references)
        .saturating_add(template.len());
    run(
        inject_command(program),
        Some(template.into_bytes()),
        limit,
        deadline,
    )
}

/// The most bytes of standard output that `starmap agent start` may print:
/// its socket, pid and lapse.
#[cfg(feature = "agent")]
const START_OUTPUT_LIMIT: usize = 4 * 1024;

/// Runs `program agent start` and returns how it ended. It states the items
/// of PROC-2 where they differ from [`op_read`].
///
/// - Program: `program`, the path of the `starmap` command that the host
///   passed to `AgentStarter::new`. The host trusts whoever installed it.
/// - Arguments: `agent` and `start`, each a separate value.
/// - Environment: cleared, and then only `environment`, which the starter
///   limits to named variables that hold no value starmap reads, such as
///   `HOME` and `STARMAP_AGENT_LAPSE`. No `OP_*` variable and no `PATH`
///   reach the agent, which never runs `op`.
/// - Streams and deadline: standard input is empty. Standard output is
///   kept up to 4 KiB and then dropped, and standard error as for
///   `op read`. `deadline` is the starter's, at most a few seconds.
/// - End: as for `op read`. `starmap agent start` returns once the agent
///   it detached with `setsid` is listening, and the agent closes the pipes
///   it inherited, so the agent outlives this operation by design. A run
///   stopped at the deadline can leave an agent that started late, which
///   the next resolution then finds.
///
/// # Errors
///
/// Returns the errors of [`op_read`].
#[cfg(feature = "agent")]
pub(crate) fn agent_start(
    program: &Path,
    environment: &[(OsString, OsString)],
    deadline: Instant,
) -> io::Result<OpRun> {
    #[expect(
        clippy::disallowed_methods,
        reason = "PROC-1: agent_start is an operation that starts starmap agent start"
    )]
    let mut command = Command::new(program);
    command
        .args(["agent", "start"])
        .env_clear()
        .envs(environment.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run(command, None, START_OUTPUT_LIMIT, deadline)
}

/// Starts `command`, writes `input` to its standard input when there is
/// one, keeps up to `limit` bytes of its standard output, and stops it as
/// [`op_read`] describes.
fn run(
    mut command: Command,
    input: Option<Vec<u8>>,
    limit: usize,
    deadline: Instant,
) -> io::Result<OpRun> {
    #[expect(
        clippy::disallowed_methods,
        reason = "PROC-1: run starts op for op_read and op_inject"
    )]
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return Ok(OpRun::NotStarted(error)),
    };
    let (sender, receiver) = mpsc::channel();
    let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    let started = stdout
        .map_or(Ok(()), |pipe| read_output(pipe, limit, sender.clone()))
        .and_then(|()| stderr.map_or(Ok(()), |pipe| read_error(pipe, sender)))
        .and_then(|()| match (stdin, input) {
            (Some(pipe), Some(input)) => write_input(pipe, input),
            _ => Ok(()),
        });
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
            Ok(Stream::Output(bytes)) if bytes.len() > limit => {
                stop(&mut child)?;
                return Ok(OpRun::TooLong);
            }
            Ok(Stream::Output(bytes)) => output = bytes,
            Ok(Stream::Error(bytes)) => error = bytes,
            Err(RecvTimeoutError::Timeout) => {
                stop(&mut child)?;
                return Ok(OpRun::Deadline);
            }
            // Both readers have sent their stream.
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = stop(&mut child)?;
    if status.success() {
        Ok(OpRun::Printed(output))
    } else {
        Ok(OpRun::Failed {
            status,
            message: String::from_utf8_lossy(&error).into_owned(),
        })
    }
}

/// Returns the command of `op read` for `reference`, which [`op_read`]
/// starts.
fn read_command(program: &Path, reference: &str) -> Command {
    #[expect(
        clippy::disallowed_methods,
        reason = "PROC-1: op_read is an operation that starts op"
    )]
    let mut command = Command::new(program);
    command
        .args(["read", "--no-newline", "--", reference])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// Returns the command of `op inject`, which [`op_inject`] starts.
fn inject_command(program: &Path) -> Command {
    #[expect(
        clippy::disallowed_methods,
        reason = "PROC-1: op_inject is an operation that starts op"
    )]
    let mut command = Command::new(program);
    command
        .arg("inject")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// Kills `child`, waits for it, and returns its exit status. The child is
/// reaped whatever the kill returns.
#[expect(
    clippy::disallowed_types,
    reason = "PROC-1: run owns its child and stops it"
)]
fn stop(child: &mut process::Child) -> io::Result<ExitStatus> {
    // A kill fails only for a child that is already reaped, which the wait
    // then reports.
    let _ = child.kill();
    child.wait()
}

/// Writes `input` to standard input on a new thread and then closes it, so
/// a large template cannot block the operation past its deadline.
fn write_input(mut pipe: impl Write + Send + 'static, input: Vec<u8>) -> io::Result<()> {
    thread::Builder::new().spawn(move || {
        // A write fails when op exits without reading its input, and the
        // exit status then reports the failure.
        let _ = pipe.write_all(&input);
    })?;
    Ok(())
}

/// Reads standard output on a new thread, up to one byte over `most`, and
/// sends it when the pipe closes or the limit is passed. The buffer holds
/// the limit from the start, so reading never moves the secret and leaves
/// a copy behind.
fn read_output(
    pipe: impl Read + Send + 'static,
    most: usize,
    sender: Sender<Stream>,
) -> io::Result<()> {
    let most = most.saturating_add(1);
    thread::Builder::new().spawn(move || {
        let mut bytes = Zeroizing::new(Vec::with_capacity(most));
        // A read that fails keeps the bytes read until then, and the exit
        // status reports the failure.
        let _ = pipe.take(limit(most)).read_to_end(&mut bytes);
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
