//! [`AgentClient`] sends one request to the agent on each connection, after
//! it checks that the process behind the socket runs as the current user.

use std::io::{self, Read as _, Write as _};
use std::os::fd::{AsFd as _, AsRawFd as _, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::sys::socket::{self, AddressFamily, SockFlag, SockType, UnixAddr, sockopt};
use starmap::{OnePasswordRef, Secret};
use zeroize::Zeroizing;

use super::protocol::{self, Expect, FRAME_LIMIT, Response};
use super::{AgentError, AgentSocket, CacheScope, Generation};

/// The longest that connecting, or writing or reading one frame, may take,
/// so an agent that stops answering cannot hold a resolution. Each wait is
/// also cut to the time left before the resolution's deadline.
const FRAME_TIMEOUT: Duration = Duration::from_secs(2);

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
            Response::Stored => Err(other_kind()),
        }
    }

    /// Stores `value` for `reference` in `scope`, read after the Get that
    /// returned `generation`, by `deadline`.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`AgentClient::get`], [`AgentError::Exists`]
    /// when the agent already holds the reference, and
    /// [`AgentError::StaleGeneration`] when the agent cleared or restarted
    /// since that Get.
    pub fn put(
        &self,
        scope: &CacheScope,
        reference: &OnePasswordRef,
        value: &Secret,
        generation: Generation,
        deadline: Instant,
    ) -> Result<(), AgentError> {
        let frame = protocol::put(scope, reference.as_str(), value, generation)?;
        match self.exchange(&frame, Expect::Stored, deadline)? {
            Response::Stored => Ok(()),
            Response::Hits(..) => Err(other_kind()),
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
        let body = send(&mut stream, request, deadline)
            .map_err(|error| AgentError::Protocol(format!("the agent did not answer: {error}")))?;
        protocol::response(&body, expect)
    }
}

/// Returns the time left before `deadline`, at most [`FRAME_TIMEOUT`], or
/// `None` once it has passed.
fn wait(deadline: Instant) -> Option<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    (!left.is_zero()).then(|| left.min(FRAME_TIMEOUT))
}

/// Connects to the socket at `path` without blocking past `deadline`. The
/// socket closes on exec, so no child that another thread starts keeps it,
/// and it is non-blocking while it connects, so a connection still in
/// progress is waited for with `poll`. Every failure reads as
/// [`AgentError::NotRunning`], because no agent could be reached.
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
            if socket::getsockopt(&stream, sockopt::SocketError).map_err(not_running)? != 0 {
                return Err(AgentError::NotRunning);
            }
        }
        Err(_) => return Err(AgentError::NotRunning),
    }
    stream
        .set_nonblocking(false)
        .map_err(|_| AgentError::NotRunning)?;
    Ok(stream)
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

/// Writes `request` and reads one frame's body by `deadline`. The body's
/// buffer is allocated at its full size, and a length over 1 MiB is refused
/// before anything is allocated.
fn send(
    stream: &mut UnixStream,
    request: &[u8],
    deadline: Instant,
) -> io::Result<Zeroizing<Vec<u8>>> {
    stream.set_write_timeout(Some(left(deadline)?))?;
    stream.write_all(request)?;
    let mut length = [0_u8; 4];
    read_by(stream, &mut length, deadline)?;
    let length = usize::try_from(u32::from_be_bytes(length)).unwrap_or(usize::MAX);
    if length > FRAME_LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the frame is over 1 MiB",
        ));
    }
    let mut body = Zeroizing::new(vec![0_u8; length]);
    read_by(stream, &mut body, deadline)?;
    Ok(body)
}

/// Fills `buffer` from `stream`, and fails once `deadline` passes. Each
/// read waits only for the time left, because a socket's read timeout
/// limits one read, and an agent that sends one byte at a time would
/// otherwise hold the client for a timeout per byte.
fn read_by(stream: &mut UnixStream, buffer: &mut [u8], deadline: Instant) -> io::Result<()> {
    let mut filled = 0;
    while let Some(rest) = buffer.get_mut(filled..).filter(|rest| !rest.is_empty()) {
        match stream.set_read_timeout(Some(left(deadline)?)) {
            // macOS refuses a new timeout with EINVAL once the agent has
            // closed its end. A read then returns at once, with what is
            // buffered or with the end of the stream, so no wait is left
            // to limit.
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::InvalidInput => {}
            Err(error) => return Err(error),
        }
        match stream.read(rest) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Returns the time left before `deadline` for one wait, at most
/// [`FRAME_TIMEOUT`], or a timed-out error once it has passed.
fn left(deadline: Instant) -> io::Result<Duration> {
    wait(deadline).ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "the deadline passed"))
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
