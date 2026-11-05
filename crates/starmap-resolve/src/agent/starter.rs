//! [`AgentStarter`] starts the agent with `starmap agent start` when no
//! agent answers.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::AgentError;
use crate::process::{self, OpRun};

/// The variables the agent receives, in an environment cleared of every
/// other. None holds a value starmap reads, and the agent never runs `op`,
/// so it needs no `OP_*` variable and no `PATH`.
const PASSED: [&str; 6] = [
    "HOME",
    "USER",
    "LOGNAME",
    "LANG",
    "XDG_STATE_HOME",
    "STARMAP_AGENT_LAPSE",
];

/// The longest a start may take, because the resolution that starts the
/// agent waits for it.
const START_DEADLINE: Duration = Duration::from_secs(3);

/// Starts `program agent start`, where `program` is the `starmap` command.
#[derive(Debug, Clone)]
pub struct AgentStarter {
    program: PathBuf,
    environment: Vec<(OsString, OsString)>,
}

impl AgentStarter {
    /// Returns a starter of `program`, the path of the `starmap` command.
    /// It keeps from `environment` only `HOME`, `USER`, `LOGNAME`, `LANG`,
    /// `XDG_STATE_HOME` and `STARMAP_AGENT_LAPSE`, which the agent then
    /// receives in an otherwise empty environment.
    #[must_use]
    pub fn new<K, V>(program: PathBuf, environment: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let environment = environment
            .into_iter()
            .filter(|(name, _)| PASSED.iter().any(|passed| name.as_ref() == *passed))
            .map(|(name, value)| (name.as_ref().to_owned(), value.as_ref().to_owned()))
            .collect();
        Self {
            program,
            environment,
        }
    }

    /// Runs `starmap agent start` and waits until it returns, by `deadline`
    /// or within three seconds, whichever comes first.
    pub(super) fn start(&self, deadline: Instant) -> Result<(), AgentError> {
        let deadline = deadline.min(Instant::now() + START_DEADLINE);
        let failed = |reason: String| Err(AgentError::StartFailed(reason));
        match process::agent_start(&self.program, &self.environment, deadline) {
            Ok(OpRun::Printed(_)) => Ok(()),
            Ok(OpRun::NotStarted(error)) | Err(error) => failed(error.to_string()),
            Ok(OpRun::Failed { status, message }) => {
                let last = message
                    .lines()
                    .map(str::trim)
                    .rfind(|line| !line.is_empty());
                failed(format!(
                    "starmap agent start exited with {status}: {}",
                    last.unwrap_or("it printed no message")
                ))
            }
            Ok(OpRun::TooLong) => failed("starmap agent start printed more than 4 KiB".to_owned()),
            Ok(OpRun::Deadline) => failed("starmap agent start did not return in time".to_owned()),
        }
    }
}
