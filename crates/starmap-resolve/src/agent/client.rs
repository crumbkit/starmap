//! [`AgentClient`] sends one request to the agent on each connection, after
//! it checks that the process behind the socket runs as the current user.

use std::os::fd::{AsFd as _, AsRawFd as _, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Instant;

use nix::errno::Errno;
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::sys::socket::{self, AddressFamily, SockFlag, SockType, UnixAddr, sockopt};
use starmap::{OnePasswordRef, Secret};

use super::protocol::{self, Expect, Response, read_frame, wait, write_frame};
use super::{AgentError, AgentSocket, AgentStatus, CacheScope, Generation, PutOutcome};

#[cfg(test)]
mod tests;

/// A client of the agent at one socket. Each call opens its own
/// connection, sends one request and reads one answer.
#[derive(Debug, Clone)]
pub struct AgentClient {
    socket: AgentSocket,
}

/// The agent's answer to one Get: for each reference of the Get, in
/// request order, its value, or `None` for a miss or for a hit past the
/// agent's budget of values in one answer, and the generation that a Put
/// of a value read afterwards carries back.
#[derive(Debug)]
pub struct Hits {
    generation: Generation,
    values: Vec<Option<Secret>>,
}

impl Hits {
    /// Returns the agent's generation when it answered.
    #[must_use]
    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// Returns the value of each reference of the Get, in order, or `None`
    /// for one that is read from `op`.
    #[must_use]
    pub fn into_values(self) -> Vec<Option<Secret>> {
        self.values
    }
}

impl AgentClient {
    /// Returns a client of the agent at `socket`.
    #[must_use]
    pub fn new(socket: AgentSocket) -> Self {
        Self { socket }
    }

    /// Returns the socket the client connects to.
    pub(super) fn socket(&self) -> &AgentSocket {
        &self.socket
    }

    /// Asks the agent for the value of each of `references` in `scope`, by
    /// `deadline`.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::NotRunning`] when no agent answers,
    /// [`AgentError::ServerUntrusted`] when the agent runs as another
    /// user, [`AgentError::TooManyReferences`] for more than 256
    /// references, the error a refusal names, and
    /// [`AgentError::Protocol`] for an answer that breaks the protocol or
    /// does not arrive by `deadline`.
    pub fn get(
        &self,
        scope: &CacheScope,
        references: &[&OnePasswordRef],
        deadline: Instant,
    ) -> Result<Hits, AgentError> {
        let texts: Vec<&str> = references
            .iter()
            .map(|reference| reference.as_str())
            .collect();
        let frame = protocol::get(scope, &texts)?;
        match self.exchange(&frame, Expect::Hits(texts.len()), deadline)? {
            Response::Hits(generation, values) => Ok(Hits { generation, values }),
            _ => Err(other_kind()),
        }
    }

    /// Stores each of `values`, a reference and its value, in `scope` with
    /// one Put, read after the Get that returned `generation`, by
    /// `deadline`. It returns the outcome of each value, in order: stored,
    /// or already held, in which case the agent keeps the value it has. No
    /// value sends nothing and returns no outcome.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`AgentClient::get`], [`AgentError::Protocol`]
    /// for a value over 64 KiB or values over 1 MiB in all, and
    /// [`AgentError::StaleGeneration`] when the agent cleared or restarted
    /// since that Get, in which case it stored none of them.
    pub fn put(
        &self,
        scope: &CacheScope,
        values: &[(&OnePasswordRef, &Secret)],
        generation: Generation,
        deadline: Instant,
    ) -> Result<Vec<PutOutcome>, AgentError> {
        if values.is_empty() {
            return Ok(Vec::new());
        }
        let texts: Vec<(&str, &Secret)> = values
            .iter()
            .map(|(reference, value)| (reference.as_str(), *value))
            .collect();
        let frame = protocol::put(scope, &texts, generation)?;
        match self.exchange(&frame, Expect::Stored(texts.len()), deadline)? {
            Response::Stored(outcomes) => Ok(outcomes),
            _ => Err(other_kind()),
        }
    }

    /// Asks the agent to drop every value by `deadline`, and returns how
    /// many it dropped.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::NotRunning`] when no agent answers, and the
    /// other errors of [`AgentClient::get`].
    pub fn clear(&self, deadline: Instant) -> Result<u32, AgentError> {
        match self.exchange(&protocol::clear(), Expect::Cleared, deadline)? {
            Response::Cleared(values) => Ok(values),
            _ => Err(other_kind()),
        }
    }

    /// Asks the agent for its status by `deadline`, or returns `None` when
    /// no agent answers.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`AgentClient::get`] except
    /// [`AgentError::NotRunning`].
    pub fn status(&self, deadline: Instant) -> Result<Option<AgentStatus>, AgentError> {
        match self.exchange(&protocol::status(), Expect::Status, deadline) {
            Ok(Response::Status(status)) => Ok(Some(status)),
            Ok(_) => Err(other_kind()),
            Err(AgentError::NotRunning) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Sends `request` on a new connection and returns the answer, once the
    /// agent is found to run as the current user, all by `deadline`.
    fn exchange(
        &self,
        request: &[u8],
        expect: Expect,
        deadline: Instant,
    ) -> Result<Response, AgentError> {
        if wait(deadline).is_none() {
            return Err(AgentError::Protocol(
                "the deadline passed before the agent was asked".to_owned(),
            ));
        }
        let mut stream = connect(self.socket.path(), deadline)?;
        if peer_uid(&stream) != Some(nix::unistd::getuid().as_raw()) {
            return Err(AgentError::ServerUntrusted);
        }
        let did_not_answer =
            |error| AgentError::Protocol(format!("the agent did not answer: {error}"));
        write_frame(&mut stream, request, deadline).map_err(did_not_answer)?;
        let body = read_frame(&mut stream, deadline).map_err(did_not_answer)?;
        protocol::response(&body, expect)
    }
}

/// Connects to the socket at `path` without blocking past `deadline`. The
/// socket closes on exec, so no child that another thread starts keeps it,
/// and it is non-blocking while it connects, so a connection still in
/// progress is waited for with `poll`. A failure reads as
/// [`connect_error`] maps it.
fn connect(path: &Path, deadline: Instant) -> Result<UnixStream, AgentError> {
    let not_running = |_| AgentError::NotRunning;
    let address = UnixAddr::new(path).map_err(not_running)?;
    let fd = close_on_exec_socket().map_err(not_running)?;
    let stream = UnixStream::from(fd);
    stream
        .set_nonblocking(true)
        .map_err(|_| AgentError::NotRunning)?;
    match socket::connect(stream.as_raw_fd(), &address) {
        Ok(()) => {}
        Err(Errno::EINPROGRESS) => {
            let left = wait(deadline).ok_or(AgentError::NotRunning)?;
            let mut fds = [PollFd::new(stream.as_fd(), PollFlags::POLLOUT)];
            let timeout = PollTimeout::try_from(left).unwrap_or(PollTimeout::MAX);
            if poll(&mut fds, timeout).map_err(not_running)? == 0 {
                return Err(AgentError::NotRunning);
            }
            let errno = socket::getsockopt(&stream, sockopt::SocketError).map_err(not_running)?;
            if errno != 0 {
                return Err(connect_error(Errno::from_raw(errno)));
            }
        }
        Err(errno) => return Err(connect_error(errno)),
    }
    stream
        .set_nonblocking(false)
        .map_err(|_| AgentError::NotRunning)?;
    Ok(stream)
}

/// Returns the error of a failed connect. Linux answers EAGAIN when the
/// listening agent's backlog is full, so that agent is running and busy,
/// and the resolver must not start a second one. Every other failure reads
/// as [`AgentError::NotRunning`], because no agent could be reached.
fn connect_error(errno: Errno) -> AgentError {
    match errno {
        Errno::EAGAIN => AgentError::Busy,
        _ => AgentError::NotRunning,
    }
}

/// Returns a new Unix stream socket that closes on exec. Linux sets the
/// flag as it creates the socket, so no fork in between can inherit it.
#[cfg(target_os = "linux")]
fn close_on_exec_socket() -> nix::Result<OwnedFd> {
    socket::socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::SOCK_CLOEXEC,
        None,
    )
}

/// Returns a new Unix stream socket that closes on exec. macOS has no
/// `SOCK_CLOEXEC`, so the flag is set right after the socket is created.
#[cfg(target_os = "macos")]
fn close_on_exec_socket() -> nix::Result<OwnedFd> {
    use nix::fcntl::{FcntlArg, FdFlag, fcntl};

    let fd = socket::socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::empty(),
        None,
    )?;
    fcntl(&fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;
    Ok(fd)
}

/// Returns the uid of the process that created the socket's other end.
#[cfg(target_os = "macos")]
fn peer_uid(stream: &UnixStream) -> Option<u32> {
    nix::unistd::getpeereid(stream)
        .ok()
        .map(|(uid, _)| uid.as_raw())
}

/// Returns the uid of the process that created the socket's other end.
#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> Option<u32> {
    socket::getsockopt(stream, sockopt::PeerCredentials)
        .ok()
        .map(|credentials| credentials.uid())
}

/// Returns the error for an answer of another kind than the request's,
/// which the parser already refuses.
fn other_kind() -> AgentError {
    AgentError::Protocol("the agent answered with another kind of frame".to_owned())
}
