//! End-to-end tests of how `starmap check` reports a `starmap.toml` that it
//! cannot read or parse. Each test runs the built binary with a cleared
//! environment and a state directory of its own.

#![cfg(test)]

use std::fs;
use std::path::Path;

use serde_json::Value;
use snapbox::cmd::{Command, OutputAssert};
use snapbox::file;
use starmap::Spec;

/// The directory that holds a `starmap.toml` fixture for each kind of spec
/// problem, relative to this package.
const SPEC_FIXTURES: &str = "../starmap/tests/spec";

/// Runs the built `starmap` binary with `args` in `dir`, with a cleared
/// environment and its state under `state`.
fn starmap(dir: impl AsRef<Path>, state: &Path, args: &[&str]) -> OutputAssert {
    Command::new(env!("CARGO_BIN_EXE_starmap"))
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("XDG_STATE_HOME", state)
        .assert()
}

/// Returns the code of the failure document that `output` holds.
fn code(output: &[u8]) -> String {
    let document: Value = serde_json::from_slice(output).unwrap();
    document["code"].as_str().unwrap().to_owned()
}

#[test]
fn a_file_with_two_guesses_prints_one_failure_that_points_at_each() {
    let state = tempfile::tempdir().unwrap();
    starmap(
        SPEC_FIXTURES,
        state.path(),
        &["check", "--spec", "guess.toml"],
    )
    .code(1)
    .stdout_eq("")
    .stderr_eq(file!["golden/check-guess.stderr"]);
}

#[test]
fn a_file_with_one_problem_states_it_on_the_error_line_and_labels_it_here() {
    let state = tempfile::tempdir().unwrap();
    starmap(
        SPEC_FIXTURES,
        state.path(),
        &["check", "--spec", "bad_default.toml"],
    )
    .code(1)
    .stdout_eq("")
    .stderr_eq(file!["golden/check-bad-default.stderr"]);
}

#[test]
fn a_file_with_two_guesses_lists_both_problems_in_json() {
    let state = tempfile::tempdir().unwrap();
    starmap(
        SPEC_FIXTURES,
        state.path(),
        &["check", "--spec", "guess.toml", "--json"],
    )
    .code(1)
    .stdout_eq(file!["golden/check-guess-json.json"]);
}

#[test]
fn a_file_over_256_kib_fails_as_too_large_without_being_parsed() {
    let dir = tempfile::tempdir().unwrap();
    let mut text = "#".repeat(Spec::MAX_BYTES);
    text.push_str("\n[variables.NOT TOML");
    fs::write(dir.path().join("starmap.toml"), text).unwrap();
    let run = starmap(dir.path(), dir.path(), &["check", "--json"]).code(1);
    let json = &run.get_output().stdout;
    assert_eq!(code(json), "starmap::spec::too_large");
    assert!(
        json.len() < 1000,
        "the JSON failure has {} bytes",
        json.len()
    );
    // A parsed file would carry an excerpt of its text, so a short failure
    // shows that the file was refused before parsing.
    let run = starmap(dir.path(), dir.path(), &["check"]).code(1);
    let stderr = &run.get_output().stderr;
    assert!(stderr.len() < 500, "the failure has {} bytes", stderr.len());
}

#[test]
fn a_file_of_exactly_256_kib_is_read_and_parsed() {
    let dir = tempfile::tempdir().unwrap();
    let mut text = "#".repeat(Spec::MAX_BYTES - 1);
    text.push('\n');
    fs::write(dir.path().join("starmap.toml"), text).unwrap();
    starmap(dir.path(), dir.path(), &["check"])
        .success()
        .stdout_eq("starmap.toml declares 0 variables.\n");
}

#[test]
fn a_file_that_is_not_utf8_fails_as_a_syntax_problem() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("starmap.toml"), b"purpose = \"\xff\"\n").unwrap();
    let run = starmap(dir.path(), dir.path(), &["check", "--json"]).code(1);
    assert_eq!(code(&run.get_output().stdout), "starmap::spec::syntax");
}

#[test]
fn a_checkout_without_starmap_toml_fails_as_not_found() {
    let dir = tempfile::tempdir().unwrap();
    starmap(dir.path(), dir.path(), &["check"])
        .code(1)
        .stderr_eq(file!["golden/check-not-found.stderr"]);
}
