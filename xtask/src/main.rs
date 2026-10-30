//! `cargo xtask` runs starmap's repository checks.
//!
//! `cargo xtask check` runs every check. This module holds every effect: it
//! reads the command line, lists and reads the repository's files, runs
//! `git`, prints the violations and sets the exit code. The checks
//! themselves are pure functions in [`check`].

mod check;

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, ExitCode};

use clap::Parser;

use crate::check::File;

#[cfg(test)]
mod tests;

/// The repository root, which is the parent of this package's directory.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// Runs starmap's repository checks.
#[derive(Debug, PartialEq, Eq, Parser)]
#[command(bin_name = "cargo xtask")]
enum Cli {
    /// Runs every repository check: the AGENTS.md word limit and the Rust
    /// file length limit.
    Check,
}

/// Runs the check that the command line selects. The exit code is 0 when the
/// checks pass, 1 when they fail or cannot run, and 2 when clap refuses the
/// command line.
fn main() -> ExitCode {
    let Cli::Check = Cli::parse();

    #[expect(
        clippy::disallowed_methods,
        reason = "main owns standard error, where xtask prints violations and failures"
    )]
    let mut stderr = io::stderr().lock();

    match check_repository(&mut stderr) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            // A failed write to standard error has nowhere else to be
            // reported, so main ignores it and the exit code still gives the
            // outcome.
            let _ = writeln!(stderr, "error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs every check over the repository and writes each violation to `out`
/// as one line. Returns whether the repository passed.
fn check_repository(out: &mut impl Write) -> io::Result<bool> {
    let violations = check::run(&read_files()?);
    for violation in &violations {
        writeln!(out, "{violation}")?;
    }
    Ok(violations.is_empty())
}

/// Reads every file that git tracks, or would track, in the repository.
fn read_files() -> io::Result<Vec<File>> {
    #[expect(
        clippy::disallowed_methods,
        reason = "this module owns xtask's process effects and runs git to list the files"
    )]
    let mut git = Command::new("git");
    #[expect(
        clippy::disallowed_methods,
        reason = "this module owns xtask's process effects and reads git's listing"
    )]
    let output = git
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .current_dir(ROOT)
        .output()
        .map_err(|error| io::Error::other(format!("cannot run git: {error}")))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "git ls-files failed: {}",
            message.trim()
        )));
    }
    let listing = String::from_utf8(output.stdout).map_err(io::Error::other)?;

    let mut files = Vec::new();
    // With -z, git separates paths with a NUL byte and does not quote them.
    for path in listing.split('\0').filter(|path| !path.is_empty()) {
        match fs::read(Path::new(ROOT).join(path)) {
            Ok(bytes) => files.push(File {
                path: path.to_owned(),
                // A file that is not UTF-8 text, such as an image, has no
                // contents, because no check reads binary data.
                contents: String::from_utf8(bytes).unwrap_or_default(),
            }),
            // git still lists a tracked file that was deleted from the working
            // tree until the deletion is staged, and a deleted file has no
            // size to check.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io::Error::other(format!("cannot read {path}: {error}"))),
        }
    }
    Ok(files)
}
