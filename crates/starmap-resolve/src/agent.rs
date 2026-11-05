//! The `agent` feature: what a client of starmap's agent needs to find it.
//! The agent is a process, run as the person, that will keep the 1Password
//! values starmap has read in memory and hand them back over a Unix socket.
//!
//! [`AgentSocket::for_user`] returns the socket's path by one convention, so
//! a sandbox such as phase's denies exactly the path the agent listens on.
//! Where a sandbox could not hide the socket, it turns the agent off.
//!
//! The feature builds on macOS and Linux only.

use std::error::Error;
use std::fmt;
use std::path::PathBuf;

mod socket;

pub use socket::AgentSocket;

/// The reason the agent is not used. Each variant has a code that
/// [`AgentError::code`] returns. A host reports it as a warning, and
/// resolution reads from `op`.
#[derive(Debug)]
#[non_exhaustive]
pub enum AgentError {
    /// The agent is not used, for the reason given.
    Off(String),
    /// The socket's directory is not the person's own, not a directory, or
    /// not mode 0700.
    UnsafeDirectory(PathBuf),
    /// The socket's path is longer than the platform allows.
    PathTooLong(PathBuf),
}

impl AgentError {
    /// Returns the error's code, such as `starmap::agent::off`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Off(_) => "starmap::agent::off",
            Self::UnsafeDirectory(_) => "starmap::agent::unsafe_directory",
            Self::PathTooLong(_) => "starmap::agent::path_too_long",
        }
    }
}

impl fmt::Display for AgentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Off(reason) => write!(formatter, "the agent is off: {reason}"),
            Self::UnsafeDirectory(path) => write!(
                formatter,
                "{} is not a directory of this user with mode 0700",
                path.display()
            ),
            Self::PathTooLong(path) => write!(
                formatter,
                "the socket path {} is longer than this platform allows",
                path.display()
            ),
        }
    }
}

impl Error for AgentError {}
