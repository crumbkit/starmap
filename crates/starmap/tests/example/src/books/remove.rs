//! Removing a book, which records its id in the state directory.

use std::env;
use std::path::PathBuf;

/// Returns the directory that holds the ids of removed books.
pub(crate) fn state_dir() -> Option<PathBuf> {
    env::var_os("XDG_STATE_HOME").map(PathBuf::from)
}
