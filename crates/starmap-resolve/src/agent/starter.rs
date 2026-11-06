//! [`AgentStarter`] starts the agent when no agent answers, with a cleared
//! environment that holds only named variables that carry no value
//! starmap reads.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use super::{AgentError, AgentSocket};
use crate::process;

/// The variables that the agent's environment keeps. The agent needs its
/// home and user to find its audit log, `LANG` for its messages,
/// `XDG_STATE_HOME` for the audit log on Linux, and `STARMAP_AGENT_LAPSE`
/// for its unlock period. It never runs `op`, so it gets no `OP_`
/// variable, no `PATH` and no credential.
const KEPT: [&str; 6] = [
    "HOME",
    "USER",
    "LOGNAME",
    "LANG",
    "XDG_STATE_HOME",
    "STARMAP_AGENT_LAPSE",
];

/// Starts `<program> agent start --socket <path>` when no agent answers, so
/// the next resolution finds one. The host passes the path of the starmap
/// program, and the environment it would otherwise give a child, of which
/// the starter keeps only the variables that the agent needs.
#[derive(Debug, Clone)]
pub struct AgentStarter {
    program: PathBuf,
    environment: Vec<(OsString, OsString)>,
}

impl AgentStarter {
    /// Returns a starter of the starmap program at `program`, which keeps
    /// only `HOME`, `USER`, `LOGNAME`, `LANG`, `XDG_STATE_HOME` and
    /// `STARMAP_AGENT_LAPSE` of `environment`.
    #[must_use]
    pub fn new<K, V>(program: PathBuf, environment: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let environment = environment
            .into_iter()
            .filter(|(name, _)| KEPT.iter().any(|kept| name.as_ref() == OsStr::new(kept)))
            .map(|(name, value)| (name.as_ref().to_owned(), value.as_ref().to_owned()))
            .collect();
        Self {
            program,
            environment,
        }
    }

    /// Starts `starmap agent start` for `socket` and returns without
    /// waiting for it. A thread of its own waits for it and stops it after
    /// 10 seconds. `starmap agent start` exits once an agent answers, and
    /// an agent that is already running makes it exit at once.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::StartFailed`] when the program cannot be
    /// started, such as a path where no program is, or when a thread to
    /// watch it cannot be started.
    pub fn start(&self, socket: &AgentSocket) -> Result<(), AgentError> {
        process::agent_start(&self.program, socket.path(), &self.environment)
            .map_err(|error| AgentError::StartFailed(error.to_string()))
    }
}
