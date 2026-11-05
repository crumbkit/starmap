//! The path of the agent's socket. [`AgentSocket::for_user`] computes it by
//! one convention, so a sandbox such as phase's denies exactly the path the
//! agent listens on, and no environment variable changes it.

use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};
use std::path::{Path, PathBuf};

use super::AgentError;

#[cfg(test)]
mod tests;

/// The directory that holds the socket, inside the person's own directory.
const DIRECTORY: &str = "starmap-agent";

/// The socket's name inside [`DIRECTORY`].
const SOCKET: &str = "agent.sock";

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
    /// in the directory of `confstr(_CS_DARWIN_USER_TEMP_DIR)` on macOS,
    /// and in `/run/user/<uid>` on Linux. `TMPDIR` and `XDG_RUNTIME_DIR`
    /// play no part. It creates `starmap-agent/` with mode 0700 when it is
    /// missing.
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

    /// Returns the socket at `path`, such as one that `--socket` names. The
    /// directory that holds it must be the user's own with mode 0700.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::UnsafeDirectory`] when the directory is not
    /// the user's own with mode 0700, and [`AgentError::PathTooLong`] when
    /// the path does not fit in `sun_path`.
    pub fn at(path: PathBuf) -> Result<Self, AgentError> {
        let uid = nix::unistd::getuid().as_raw();
        let directory = path.parent().unwrap_or(Path::new("."));
        check_directory(directory, uid)?;
        Self::fitted(path)
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
    super::darwin::user_temp_dir().ok_or_else(|| {
        AgentError::Off("macOS gave no temporary directory for this user".to_owned())
    })
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
