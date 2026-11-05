//! The path of the agent's socket. [`AgentSocket::for_user`] computes it by
//! one convention, so a sandbox such as phase's denies exactly the path the
//! agent listens on, and no environment variable changes it.

#[cfg(target_os = "macos")]
use std::ffi::OsStr;
use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::sync::OnceLock;
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

use super::AgentError;
#[cfg(target_os = "macos")]
use crate::process::{self, OpRun};

#[cfg(test)]
mod tests;

/// The directory that holds the socket, inside the person's own directory.
const DIRECTORY: &str = "starmap-agent";

/// The socket's name inside [`DIRECTORY`].
const SOCKET: &str = "agent.sock";

/// How long `getconf` may take to print the user's temporary directory.
#[cfg(target_os = "macos")]
const GETCONF_DEADLINE: Duration = Duration::from_secs(2);

/// The size of `sun_path`, which holds the socket's path and a NUL.
#[cfg(target_os = "macos")]
const SUN_PATH: usize = 104;
#[cfg(target_os = "linux")]
const SUN_PATH: usize = 108;

/// The path of the agent's socket, checked to lie in a directory that only
/// its owner can open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSocket {
    path: PathBuf,
}

impl AgentSocket {
    /// Returns the socket of the current user: `starmap-agent/agent.sock`
    /// in the directory that `getconf DARWIN_USER_TEMP_DIR` prints on macOS,
    /// and in `/run/user/<uid>` on Linux. `TMPDIR` and `XDG_RUNTIME_DIR`
    /// play no part. It creates `starmap-agent/` with mode 0700 when it is
    /// missing. On macOS `getconf` runs once per process, and later calls
    /// reuse its answer.
    ///
    /// The macOS path starts with `/var`, which is a symbolic link, so a
    /// sandbox has to deny the resolved `/private/var/…` path.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::Off`] when the parent directory is missing,
    /// is not a directory the user owns, can be written by group or
    /// others, or has the sticky bit of a shared directory such as `/tmp`,
    /// and on Linux when the kernel's Landlock cannot deny a sandboxed app
    /// the socket. Returns [`AgentError::UnsafeDirectory`] when
    /// `starmap-agent/` cannot be created or is not the user's own with
    /// mode 0700, and [`AgentError::PathTooLong`] when the path does not
    /// fit in `sun_path`.
    pub fn for_user() -> Result<Self, AgentError> {
        let uid = nix::unistd::getuid().as_raw();
        Self::in_parent(&user_parent(uid)?, uid)
    }

    /// Returns the socket's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the socket in `parent/starmap-agent/` for `uid`, after the
    /// checks [`AgentSocket::for_user`] lists.
    fn in_parent(parent: &Path, uid: u32) -> Result<Self, AgentError> {
        check_parent(parent, uid)?;
        let directory = parent.join(DIRECTORY);
        match DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(AgentError::UnsafeDirectory(directory)),
        }
        check_directory(&directory, uid)?;
        Self::fitted(directory.join(SOCKET))
    }

    /// Returns the socket at `path` when the path fits in `sun_path` with
    /// its NUL.
    fn fitted(path: PathBuf) -> Result<Self, AgentError> {
        if path.as_os_str().as_bytes().len() >= SUN_PATH {
            return Err(AgentError::PathTooLong(path));
        }
        Ok(Self { path })
    }
}

/// Returns the directory of `uid`'s own files that holds `starmap-agent/`.
#[cfg(target_os = "macos")]
fn user_parent(_uid: u32) -> Result<PathBuf, AgentError> {
    // The directory does not change while the process runs, so getconf runs
    // once, and a failure is kept too, so the agent stays off.
    static ANSWER: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    ANSWER
        .get_or_init(user_temp_dir)
        .clone()
        .map_err(|reason| AgentError::Off(format!("getconf DARWIN_USER_TEMP_DIR {reason}")))
}

/// Returns the directory that `getconf DARWIN_USER_TEMP_DIR` prints, or the
/// reason it gave none.
#[cfg(target_os = "macos")]
fn user_temp_dir() -> Result<PathBuf, String> {
    let deadline = Instant::now() + GETCONF_DEADLINE;
    match process::getconf_user_temp_dir(deadline) {
        Ok(OpRun::Printed(output)) => {
            let text = output.strip_suffix(b"\n").unwrap_or(&output);
            if text.first() == Some(&b'/') {
                Ok(PathBuf::from(OsStr::from_bytes(text)))
            } else {
                Err("printed no absolute path".to_owned())
            }
        }
        Ok(OpRun::NotStarted(error)) | Err(error) => Err(format!("failed: {error}")),
        Ok(OpRun::Failed { status, .. }) => Err(format!("exited with {status}")),
        Ok(OpRun::TooLong) => Err("printed more than 4 KiB".to_owned()),
        Ok(OpRun::Deadline) => Err("did not answer in time".to_owned()),
    }
}

/// Returns the directory of `uid`'s own files that holds `starmap-agent/`,
/// once the kernel's Landlock can deny a sandboxed app the socket.
#[cfg(target_os = "linux")]
fn user_parent(uid: u32) -> Result<PathBuf, AgentError> {
    if !landlock_denies_sockets() {
        return Err(AgentError::Off(
            "this kernel's Landlock cannot deny a sandboxed app the socket".to_owned(),
        ));
    }
    Ok(PathBuf::from(format!("/run/user/{uid}")))
}

/// Whether the running kernel's Landlock governs connecting to a pathname
/// Unix socket, which its ABI 9 added. Creating a ruleset restricts
/// nothing, so the check leaves the process as it was.
#[cfg(target_os = "linux")]
fn landlock_denies_sockets() -> bool {
    use landlock::{AccessFs, CompatLevel, Compatible as _, Ruleset, RulesetAttr as _};

    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::ResolveUnix)
        .and_then(Ruleset::create)
        .is_ok()
}

/// Checks that `parent` is a real directory that `uid` owns, that group
/// and others cannot write, and that has no sticky bit.
fn check_parent(parent: &Path, uid: u32) -> Result<(), AgentError> {
    let private = fs::symlink_metadata(parent).is_ok_and(|metadata| {
        metadata.file_type().is_dir() && metadata.uid() == uid && metadata.mode() & 0o1022 == 0
    });
    if private {
        Ok(())
    } else {
        Err(AgentError::Off(format!(
            "{} is not a private directory of this user",
            parent.display()
        )))
    }
}

/// Checks that `directory` is a real directory that `uid` owns with mode
/// 0700.
fn check_directory(directory: &Path, uid: u32) -> Result<(), AgentError> {
    let private = fs::symlink_metadata(directory).is_ok_and(|metadata| {
        metadata.file_type().is_dir() && metadata.uid() == uid && metadata.mode() & 0o7777 == 0o700
    });
    if private {
        Ok(())
    } else {
        Err(AgentError::UnsafeDirectory(directory.to_owned()))
    }
}
