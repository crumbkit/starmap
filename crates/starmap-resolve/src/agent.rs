//! The `agent` feature: the client side of starmap's agent, a process run
//! as the person that keeps the 1Password values starmap has read in
//! memory and hands them back over a Unix socket.
//!
//! [`AgentSocket::for_user`] returns the socket's path by one convention, so
//! a sandbox such as phase's denies exactly the path the agent listens on.
//! Where a sandbox could not hide the socket, it turns the agent off.
//!
//! The agent is a cache and never runs `op`. [`Agent`] is a
//! [`crate::Resolver`] that asks the agent for each lookup that
//! [`crate::resolve`] marked [`crate::Cacheable`], reads every other lookup
//! and every miss with one batched `op` call of its own, so the 1Password
//! prompt names the program that wants the value, and then puts what it
//! read back into the agent. [`CacheScope`] names the account and `op`
//! program a cache entry belongs to, and [`AgentClient`] sends one request
//! per connection. [`AgentStarter`] starts an agent when none answers.
//!
//! [`protocol`] is the wire protocol. It holds the client's half, and
//! behind the `agent-server` feature the agent's half, so the agent's own
//! crate parses requests and writes answers with the same code.
//!
//! The feature builds on macOS and Linux only.

use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

mod client;
pub mod protocol;
mod resolver;
mod socket;
mod starter;

#[cfg(test)]
mod tests;

pub use client::{AgentClient, Hits};
pub use resolver::Agent;
pub use socket::AgentSocket;
pub use starter::AgentStarter;

/// The variable that names the 1Password account `op` uses.
const ACCOUNT: &str = "OP_ACCOUNT";

/// The variable of a 1Password service account's token. A service account
/// never prompts, so the agent would add exposure and save nothing.
const SERVICE_ACCOUNT: &str = "OP_SERVICE_ACCOUNT_TOKEN";

/// The prefix of the variables that hold a session `op signin` printed. A
/// session does not prompt either.
const SESSION_PREFIX: &str = "OP_SESSION_";

/// The agent's generation: an opaque token that a Get returns and a Put
/// carries back unchanged, so the agent refuses a value read before its
/// latest clear or before it restarted. The protocol module says how the
/// agent draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Generation(u64);

impl Generation {
    /// Returns a new generation of 64 bits from the operating system's
    /// random source. The agent draws one when it starts and another on
    /// every clear, so a Put that carries a generation from before either
    /// never matches.
    ///
    /// The generation needs to differ between instances and clears, and it
    /// need not be secret, because any client can read it with a Get. When
    /// the random source fails, which the operating system does not do in
    /// practice, the generation mixes the wall clock's nanoseconds with the
    /// process id instead.
    #[must_use]
    pub fn draw() -> Self {
        let mut bytes = [0_u8; 8];
        if getrandom::fill(&mut bytes).is_ok() {
            return Self(u64::from_be_bytes(bytes));
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let nanos = u64::try_from(nanos & u128::from(u64::MAX)).unwrap_or(u64::MAX);
        Self(nanos ^ u64::from(process::id()).rotate_left(32))
    }
}

/// Whether the agent stored one value of a Put.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PutOutcome {
    /// The agent stored the value.
    Stored,
    /// The agent already held a value for the reference and kept it.
    AlreadyHeld,
}

/// What the agent reports about itself: its generation, how many values it
/// holds, its process id, and when its unlock period began and when it
/// lapses. It names no reference and holds no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentStatus {
    generation: Generation,
    values: u32,
    pid: u32,
    unlocked_at: Option<SystemTime>,
    lapses_at: Option<SystemTime>,
}

impl AgentStatus {
    /// Returns the status of an agent at `generation` that holds `values`
    /// values and runs as process `pid`. `unlocked_at` and `lapses_at` are
    /// `None` while it holds no value. The wire carries both in whole Unix
    /// seconds, so a status read from the agent drops any fraction.
    #[must_use]
    pub fn new(
        generation: Generation,
        values: u32,
        pid: u32,
        unlocked_at: Option<SystemTime>,
        lapses_at: Option<SystemTime>,
    ) -> Self {
        Self {
            generation,
            values,
            pid,
            unlocked_at,
            lapses_at,
        }
    }

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

    /// Returns the agent's process id.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Returns when the agent's unlock period began, or `None` while it
    /// holds no value.
    #[must_use]
    pub fn unlocked_at(&self) -> Option<SystemTime> {
        self.unlocked_at
    }

    /// Returns when the agent's values lapse, or `None` while it holds no
    /// value.
    #[must_use]
    pub fn lapses_at(&self) -> Option<SystemTime> {
        self.lapses_at
    }
}

/// The account and `op` program a cache entry belongs to, so two accounts,
/// or two `op` programs, never share an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheScope {
    account: String,
    op: PathBuf,
}

impl CacheScope {
    /// Returns the scope of `op`, the absolute path of the 1Password CLI,
    /// run with `environment`, the variables it inherits. The account is
    /// the value of `OP_ACCOUNT`.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::Off`] when `op` is not an absolute path, when
    /// `OP_ACCOUNT` is not set to a non-empty UTF-8 value, because `op`'s
    /// default account can change between runs and two accounts would then
    /// share entries, and when `OP_SERVICE_ACCOUNT_TOKEN` or any
    /// `OP_SESSION_` variable is set, because those sessions never prompt.
    /// The host then reads from `op` directly.
    pub fn new<K, V>(
        op: PathBuf,
        environment: impl IntoIterator<Item = (K, V)>,
    ) -> Result<Self, AgentError>
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        if !op.is_absolute() {
            return Err(AgentError::Off(format!(
                "the op program {} is not an absolute path",
                op.display()
            )));
        }
        let mut account = String::new();
        for (name, value) in environment {
            let name = name.as_ref().to_string_lossy();
            if name == SERVICE_ACCOUNT || name.starts_with(SESSION_PREFIX) {
                return Err(AgentError::Off(format!(
                    "{name} is set, so op does not prompt"
                )));
            }
            if name == ACCOUNT {
                value
                    .as_ref()
                    .to_str()
                    .unwrap_or_default()
                    .clone_into(&mut account);
            }
        }
        if account.is_empty() {
            return Err(AgentError::Off(format!(
                "{ACCOUNT} is not set to an account, so op's default account could change"
            )));
        }
        Ok(Self { account, op })
    }

    /// Returns the 1Password account.
    #[must_use]
    pub fn account(&self) -> &str {
        &self.account
    }

    /// Returns the absolute path of the `op` program.
    #[must_use]
    pub fn op(&self) -> &Path {
        &self.op
    }
}

/// The reason the agent was not used or refused a request. Each variant has
/// a code that [`AgentError::code`] returns, and its message names no
/// value. A host reports it as a warning, and resolution reads from `op`.
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
    /// No agent answered at the socket.
    NotRunning,
    /// The agent refused a client of another uid.
    PeerRefused,
    /// The process behind the socket runs as another uid.
    ServerUntrusted,
    /// The agent's answer was malformed or did not arrive in time, as
    /// given.
    Protocol(String),
    /// A Put carried a generation from before the agent's latest clear or
    /// restart.
    StaleGeneration,
    /// The agent already serves as many connections as it allows.
    Busy,
    /// A Get or a Put named more references than one request may hold.
    TooManyReferences,
    /// The starter could not run `starmap agent start`, for the reason
    /// given.
    StartFailed(String),
}

impl AgentError {
    /// Returns the error's code, such as `starmap::agent::off`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Off(_) => "starmap::agent::off",
            Self::UnsafeDirectory(_) => "starmap::agent::unsafe_directory",
            Self::PathTooLong(_) => "starmap::agent::path_too_long",
            Self::NotRunning => "starmap::agent::not_running",
            Self::PeerRefused => "starmap::agent::peer_refused",
            Self::ServerUntrusted => "starmap::agent::server_untrusted",
            Self::Protocol(_) => "starmap::agent::protocol",
            Self::StaleGeneration => "starmap::agent::stale_generation",
            Self::Busy => "starmap::agent::busy",
            Self::TooManyReferences => "starmap::agent::too_many_references",
            Self::StartFailed(_) => "starmap::agent::start_failed",
        }
    }

    /// Returns the error that the agent's refusal `code` names. A code this
    /// client does not know is a protocol error, and its text is not kept,
    /// because the agent wrote it.
    fn refused(code: &str) -> Self {
        match code {
            "starmap::agent::protocol" => {
                Self::Protocol("the agent refused the request as malformed".to_owned())
            }
            "starmap::agent::peer_refused" => Self::PeerRefused,
            "starmap::agent::stale_generation" => Self::StaleGeneration,
            "starmap::agent::busy" => Self::Busy,
            "starmap::agent::too_many_references" => Self::TooManyReferences,
            _ => {
                Self::Protocol("the agent refused with a code this client does not know".to_owned())
            }
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
            Self::NotRunning => formatter.write_str("no agent answered at the socket"),
            Self::PeerRefused => formatter.write_str("the agent refused a client of another user"),
            Self::ServerUntrusted => {
                formatter.write_str("the process behind the agent's socket runs as another user")
            }
            Self::Protocol(reason) => write!(formatter, "the agent's answer is unusable: {reason}"),
            Self::StaleGeneration => {
                formatter.write_str("the value was read before the agent's latest clear or restart")
            }
            Self::Busy => formatter.write_str("the agent is serving too many connections"),
            Self::TooManyReferences => {
                formatter.write_str("one request may name at most 256 references")
            }
            Self::StartFailed(reason) => {
                write!(formatter, "starmap agent start could not run: {reason}")
            }
        }
    }
}

impl Error for AgentError {}
