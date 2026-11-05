//! [`AgentClient`] sends one request to the agent on each connection, after
//! it checks that the process behind the socket runs as the current user.

use std::io::{self, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use starmap::{OnePasswordRef, Secret};
use zeroize::Zeroizing;

use super::protocol::{self, Expect, FRAME_LIMIT, Response};
use super::{AgentError, AgentSocket, CacheScope, Generation};

/// How long one frame may take to write or to read, so an agent that stops
/// answering cannot hold a resolution.
const FRAME_TIMEOUT: Duration = Duration::from_secs(2);

/// A client of the agent at one socket. Each call opens its own
/// connection, sends one request and reads one answer.
#[derive(Debug, Clone)]
pub struct AgentClient {
    socket: AgentSocket,
}

/// The values of one Get, in request order, and the generation a Put of a
/// miss carries.
#[derive(Debug)]
pub struct Hits {
    generation: Generation,
    values: Vec<Option<Secret<String>>>,
}

impl Hits {
    /// Returns the agent's generation when it answered.
    #[must_use]
    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// Returns the value of each reference of the Get, in order, or `None`
    /// for a miss, which is read from `op`.
    #[must_use]
    pub fn into_values(self) -> Vec<Option<Secret<String>>> {
        self.values
    }
}

/// What the agent holds, without naming any reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentStatus {
    pub(super) generation: Generation,
    pub(super) values: u32,
    pub(super) unlocked_at: i64,
    pub(super) lapses_at: i64,
}

impl AgentStatus {
    /// Returns the agent's generation.
    #[must_use]
    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// Returns how many values the agent holds.
    #[must_use]
    pub fn values(&self) -> u32 {
        self.values
    }

    /// Returns when the unlock period began, in Unix seconds, or 0 when the
    /// agent holds nothing.
    #[must_use]
    pub fn unlocked_at(&self) -> i64 {
        self.unlocked_at
    }

    /// Returns when every value lapses, in Unix seconds, or 0 when the
    /// agent holds nothing.
    #[must_use]
    pub fn lapses_at(&self) -> i64 {
        self.lapses_at
    }
}

/// What a Clear dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cleared {
    values: u32,
}

impl Cleared {
    /// Returns how many values the agent dropped.
    #[must_use]
    pub fn values(&self) -> u32 {
        self.values
    }
}

impl AgentClient {
    /// Returns a client of the agent at `socket`.
    #[must_use]
    pub fn new(socket: AgentSocket) -> Self {
        Self { socket }
    }

    /// Asks the agent for the value of each of `references` in `scope`.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::NotRunning`] when no agent answers,
    /// [`AgentError::ServerUntrusted`] when the agent runs as another
    /// user, [`AgentError::TooManyReferences`] for more than 256
    /// references, the error a refusal names, and
    /// [`AgentError::Protocol`] for an answer that breaks the protocol or
    /// does not arrive in time.
    pub fn get(
        &self,
        scope: &CacheScope,
        references: &[&OnePasswordRef],
    ) -> Result<Hits, AgentError> {
        let texts: Vec<&str> = references
            .iter()
            .map(|reference| reference.as_str())
            .collect();
        match self.exchange(&protocol::get(scope, &texts)?, Expect::Hits(texts.len()))? {
            Response::Hits(generation, values) => Ok(Hits { generation, values }),
            _ => Err(other_kind()),
        }
    }

    /// Stores `value` for `reference` in `scope`, read after the Get that
    /// returned `generation`.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`AgentClient::get`], [`AgentError::Exists`]
    /// when the agent already holds the reference, and
    /// [`AgentError::StaleGeneration`] when the agent cleared since that
    /// Get.
    pub fn put(
        &self,
        scope: &CacheScope,
        reference: &OnePasswordRef,
        value: &Secret<String>,
        generation: Generation,
    ) -> Result<(), AgentError> {
        let frame = protocol::put(scope, reference.as_str(), value, generation)?;
        match self.exchange(&frame, Expect::Stored)? {
            Response::Stored => Ok(()),
            _ => Err(other_kind()),
        }
    }

    /// Asks the agent to drop and zero every value and stop.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`AgentClient::get`].
    pub fn clear(&self) -> Result<Cleared, AgentError> {
        match self.exchange(&protocol::clear(), Expect::Cleared)? {
            Response::Cleared(values) => Ok(Cleared { values }),
            _ => Err(other_kind()),
        }
    }

    /// Returns what the agent holds, or `None` when no agent answers.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`AgentClient::get`] other than
    /// [`AgentError::NotRunning`].
    pub fn status(&self) -> Result<Option<AgentStatus>, AgentError> {
        match self.exchange(&protocol::status(), Expect::Status) {
            Ok(Response::Status(status)) => Ok(Some(status)),
            Ok(_) => Err(other_kind()),
            Err(AgentError::NotRunning) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Sends `request` on a new connection and returns the answer, once the
    /// agent is found to run as the current user.
    fn exchange(&self, request: &[u8], expect: Expect) -> Result<Response, AgentError> {
        let mut stream =
            UnixStream::connect(self.socket.path()).map_err(|_| AgentError::NotRunning)?;
        if peer_uid(&stream) != Some(nix::unistd::getuid().as_raw()) {
            return Err(AgentError::ServerUntrusted);
        }
        let body = send(&mut stream, request)
            .map_err(|error| AgentError::Protocol(format!("the agent did not answer: {error}")))?;
        protocol::response(&body, expect)
    }
}

/// Writes `request` and reads one frame's body, each within
/// [`FRAME_TIMEOUT`]. The body's buffer is allocated at its full size, and
/// a length over 1 MiB is refused before anything is allocated.
fn send(stream: &mut UnixStream, request: &[u8]) -> io::Result<Zeroizing<Vec<u8>>> {
    stream.set_write_timeout(Some(FRAME_TIMEOUT))?;
    stream.set_read_timeout(Some(FRAME_TIMEOUT))?;
    stream.write_all(request)?;
    let mut length = [0_u8; 4];
    stream.read_exact(&mut length)?;
    let length = usize::try_from(u32::from_be_bytes(length)).unwrap_or(usize::MAX);
    if length > FRAME_LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the frame is over 1 MiB",
        ));
    }
    let mut body = Zeroizing::new(vec![0_u8; length]);
    stream.read_exact(&mut body)?;
    Ok(body)
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
    nix::sys::socket::getsockopt(stream, nix::sys::socket::sockopt::PeerCredentials)
        .ok()
        .map(|credentials| credentials.uid())
}

/// Returns the error for an answer of another kind than the request's,
/// which the parser already refuses.
fn other_kind() -> AgentError {
    AgentError::Protocol("the agent answered with another kind of frame".to_owned())
}
