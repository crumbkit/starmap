//! End-to-end tests of `starmap diff`. Each test builds a git repository in
//! a temporary directory, commits a starmap.toml there, changes the file in
//! the working tree and runs the built binary with a cleared environment.

#![cfg(test)]

use std::fs;
use std::path::Path;

use snapbox::cmd::{Command, OutputAssert};
use snapbox::file;
use tempfile::TempDir;

/// The spec that the repository commits on `main`.
const WAS: &str = r#"
[variables.SHELF_DEBUG]
purpose = "Logs each catalogue query."
type = "boolean"
unset = { default = "false" }
show = "shown"
source = "setting"

[variables.SHELF_PAGE_SIZE]
purpose = "How many books one page of a listing holds."
type = { integer = { min = 1, max = 50 } }
unset = { default = "20" }
show = "shown"
source = "setting"

[variables.SHELF_PORT]
purpose = "The port the api process listens on."
type = "port"
unset = { default = "8080" }
show = "shown"
source = "setting"

[variables.SHELF_VERBOSE]
purpose = "How much shelf logs."
type = { enumeration = ["quiet", "loud"] }
unset = "optional"
show = "shown"
source = "setting"
"#;

/// The spec in the working tree, which adds, changes and removes one
/// variable each and keeps `SHELF_DEBUG` as it was.
const NOW: &str = r#"
[variables.SHELF_DEBUG]
purpose = "Logs each catalogue query."
type = "boolean"
unset = { default = "false" }
show = "shown"
source = "setting"

[variables.SHELF_MIRROR_TOKEN]
purpose = "The token shelf sends to the catalogue mirror."
type = "string"
unset = "optional"
show = "withheld"
source = "secret"

[variables.SHELF_PAGE_SIZE]
purpose = "How many books one page of a listing holds."
type = { integer = { min = 1, max = 100 } }
unset = { default = "20" }
show = "shown"
source = "setting"

[variables.SHELF_PORT]
purpose = "The port the api process listens on."
type = "port"
unset = "required"
show = "shown"
source = "setting"
"#;

/// Returns the search path of the process that runs the tests, which the
/// binary needs to find git.
fn path() -> std::ffi::OsString {
    #[expect(
        clippy::disallowed_methods,
        reason = "the test passes its own PATH to the binary, so the binary finds git"
    )]
    std::env::var_os("PATH").unwrap_or_default()
}

/// Runs `program` with `args` in `dir`, with a cleared environment that
/// holds only the search path, and stops git from reading any repository
/// above `dir` or any configuration of the person who runs the tests.
fn run(program: &str, dir: &Path, args: &[&str]) -> OutputAssert {
    Command::new(program)
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", path())
        .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", dir.join("no-gitconfig"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .assert()
}

/// Runs the built `starmap` binary with `args` in `dir`.
fn starmap(dir: &Path, args: &[&str]) -> OutputAssert {
    run(env!("CARGO_BIN_EXE_starmap"), dir, args)
}

/// Runs git with `args` in `dir`, and asserts that it succeeds.
fn git(dir: &Path, args: &[&str]) {
    run("git", dir, args).success();
}

/// Returns a repository whose branch `main` has one commit that holds
/// `committed` as starmap.toml, or holds a README without a spec when
/// `committed` is `None`. The working tree's starmap.toml is [`NOW`].
fn repository(committed: Option<&str>) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();
    git(path, &["init", "--quiet", "--initial-branch", "main"]);
    match committed {
        Some(text) => fs::write(path.join("starmap.toml"), text).unwrap(),
        None => fs::write(path.join("README"), "shelf\n").unwrap(),
    }
    git(path, &["add", "."]);
    git(
        path,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--quiet",
            "--message",
            "First",
        ],
    );
    fs::write(path.join("starmap.toml"), NOW).unwrap();
    dir
}

#[test]
fn diff_lists_each_variable_that_was_added_changed_or_removed() {
    let dir = repository(Some(WAS));
    starmap(dir.path(), &["diff", "main"])
        .success()
        .stdout_eq(file!["golden/diff/starmap-diff.txt"])
        .stderr_eq(file!["golden/diff/starmap-diff.stderr"]);
}

#[test]
fn diff_prints_each_change_as_json() {
    let dir = repository(Some(WAS));
    starmap(dir.path(), &["diff", "main", "--json"])
        .success()
        .stdout_eq(file!["golden/diff/starmap-diff-json.json"]);
}

#[test]
fn diff_exits_with_success_when_nothing_changed() {
    let dir = repository(Some(NOW));
    starmap(dir.path(), &["diff", "main"])
        .success()
        .stdout_eq("")
        .stderr_eq(file!["golden/diff/starmap-diff-same.stderr"]);
}

#[test]
fn every_variable_counts_as_added_when_the_revision_has_no_spec() {
    let dir = repository(None);
    starmap(dir.path(), &["diff", "main"])
        .success()
        .stdout_eq(file!["golden/diff/starmap-diff-no-spec.txt"])
        .stderr_eq(file!["golden/diff/starmap-diff-no-spec.stderr"]);
}

#[test]
fn a_revision_that_does_not_exist_is_a_failure() {
    let dir = repository(Some(WAS));
    starmap(dir.path(), &["diff", "no-such-branch"])
        .code(1)
        .stdout_eq("")
        .stderr_eq(file!["golden/diff/starmap-diff-not-found.stderr"]);
}

#[test]
fn diff_outside_a_git_repository_is_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("starmap.toml"), NOW).unwrap();
    starmap(dir.path(), &["diff", "main"])
        .code(1)
        .stdout_eq("")
        .stderr_eq(file!["golden/diff/starmap-diff-no-repository.stderr"]);
}
