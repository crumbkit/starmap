//! The one module of the `starmap` command that starts a process (PROC-1).
//! Its one operation is [`read_at`], which reads one file as a revision of
//! a git repository holds it, for `starmap diff`.
//!
//! Each git run states the five items of PROC-2.
//!
//! - Program: `git`, found on the `PATH` of the process that runs starmap.
//!   The person who runs starmap chooses the `PATH` and trusts whoever
//!   installed the git it names.
//! - Arguments: fixed words and the revision, each a separate value. No
//!   shell parses them. A revision that starts with `-` is refused before
//!   git runs, so git never reads it as an option.
//! - Environment: the child inherits the whole environment of the process
//!   that runs starmap, so git reads the repository and configuration that
//!   the person's own `git` commands read, such as `GIT_DIR` and `HOME`.
//! - Streams and deadline: standard input is empty, so git cannot wait for
//!   typed input. Standard output and standard error are captured whole.
//!   Each run prints at most one object id or one tree entry, except
//!   `git cat-file blob`, which runs only after `git ls-tree` reports that
//!   the blob is at most [`Spec::MAX_BYTES`]. A run has no deadline: git
//!   reads only the local repository, and starmap waits for it to exit.
//! - End: `Command::output` waits for git to exit and reaps it, and the
//!   exit status decides the result. The commands only read the
//!   repository, so a run that stops early leaves nothing behind.

use std::io;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use starmap::Spec;

#[cfg(test)]
mod tests;

/// What a revision holds at a path.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct At {
    /// The full id of the commit that the revision names.
    pub(crate) commit: String,
    /// What the commit holds at the path.
    pub(crate) entry: Entry,
}

/// What a commit holds at a path.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    /// No file is at the path.
    Missing,
    /// The path names something other than a file, such as a directory.
    NotFile,
    /// The file is over [`Spec::MAX_BYTES`], so it is not read.
    TooLarge,
    /// The file's bytes.
    File(Vec<u8>),
}

/// Why [`read_at`] cannot read the path.
#[derive(Debug)]
pub(crate) enum GitError {
    /// `dir` is not in a git repository.
    NoRepository,
    /// The revision names no commit in the repository.
    NotFound,
    /// git cannot start, or fails for another reason. The error holds what
    /// git printed on standard error.
    Failed(io::Error),
}

/// Returns what `revision` holds at `name`, a file name in `dir`. git runs
/// in `dir`, so the repository is the one that holds `dir`, and `name` is
/// relative to it.
///
/// # Errors
///
/// Returns [`GitError::NoRepository`] when `dir` is in no git repository,
/// [`GitError::NotFound`] when `revision` names no commit, and
/// [`GitError::Failed`] when git cannot start or fails otherwise.
pub(crate) fn read_at(dir: &Path, revision: &str, name: &str) -> Result<At, GitError> {
    if revision.starts_with('-') {
        return Err(GitError::NotFound);
    }
    let found = git(dir, &["rev-parse", "--git-dir"]).map_err(GitError::Failed)?;
    if !found.status.success() {
        return Err(GitError::NoRepository);
    }
    let commit = format!("{revision}^{{commit}}");
    let verified = git(dir, &["rev-parse", "--verify", "--quiet", &commit]);
    let verified = succeeded(verified.map_err(GitError::Failed)?).ok_or(GitError::NotFound)?;
    let commit = text(&verified.stdout).trim().to_owned();
    let listed = checked(
        "git ls-tree",
        git(dir, &["ls-tree", "-l", "-z", &commit, "--", name]),
    )?;
    let entry = match tree_entry(&listed.stdout) {
        None => Entry::Missing,
        Some(entry) if entry.kind != "blob" => Entry::NotFile,
        Some(entry) if entry.size.is_none_or(|size| size > Spec::MAX_BYTES) => Entry::TooLarge,
        Some(entry) => {
            let read = checked("git cat-file", git(dir, &["cat-file", "blob", entry.id]))?;
            Entry::File(read.stdout)
        }
    };
    Ok(At { commit, entry })
}

/// Runs git with `args` in `dir`, with empty standard input, and returns
/// what it printed and its exit status.
fn git(dir: &Path, args: &[&str]) -> io::Result<Output> {
    #[expect(
        clippy::disallowed_methods,
        reason = "crates/starmap-cli/src/git.rs is the module that starts git"
    )]
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output();
    output
}

/// Returns `output` when git exited with success.
fn succeeded(output: Output) -> Option<Output> {
    output.status.success().then_some(output)
}

/// Returns the output of the git run `command` when it exited with
/// success, and otherwise its failure with what it printed on standard
/// error.
fn checked(command: &str, run: io::Result<Output>) -> Result<Output, GitError> {
    let output = run.map_err(GitError::Failed)?;
    if output.status.success() {
        return Ok(output);
    }
    let printed = text(&output.stderr);
    let printed = printed.trim();
    let message = if printed.is_empty() {
        format!("{command} failed")
    } else {
        format!("{command} failed: {printed}")
    };
    Err(GitError::Failed(io::Error::other(message)))
}

/// Returns `bytes` as text, with each byte that is not UTF-8 replaced.
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// One entry that `git ls-tree -l -z` prints.
#[derive(Debug, PartialEq, Eq)]
struct TreeEntry<'a> {
    /// The kind of object, such as `blob` or `tree`.
    kind: &'a str,
    /// The object's id.
    id: &'a str,
    /// The blob's size in bytes, or `None` for an object that is not a
    /// blob, which git prints as `-`.
    size: Option<usize>,
}

/// Returns the first entry of the output of `git ls-tree -l -z`, or `None`
/// when it lists none. Each entry is `<mode> <kind> <id> <size>`, a tab,
/// the path and a NUL byte.
fn tree_entry(listed: &[u8]) -> Option<TreeEntry<'_>> {
    let line = listed.split(|&byte| byte == 0).next()?;
    let head = line.split(|&byte| byte == b'\t').next()?;
    let head = std::str::from_utf8(head).ok()?;
    let mut words = head.split_whitespace();
    let (_mode, kind, id, size) = (words.next()?, words.next()?, words.next()?, words.next()?);
    Some(TreeEntry {
        kind,
        id,
        size: size.parse().ok(),
    })
}
