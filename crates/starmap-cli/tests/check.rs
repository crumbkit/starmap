//! End-to-end tests of how `starmap check` compares the spec with the
//! variables that src reads. Each test runs the built binary in a fixture
//! with a cleared environment and a state directory of its own.

#![cfg(test)]

use serde_json::{Value, json};
use snapbox::cmd::{Command, OutputAssert};
use snapbox::file;

/// The directory that holds an application for each failure of its
/// source, relative to this package.
const SOURCE_FIXTURES: &str = "tests/fixtures";

/// Runs the built `starmap` binary with `args` in [`SOURCE_FIXTURES`],
/// with a cleared environment and its usage log in a temporary directory.
fn starmap(args: &[&str]) -> OutputAssert {
    let state = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_starmap"))
        .args(args)
        .current_dir(SOURCE_FIXTURES)
        .env_clear()
        .env("XDG_STATE_HOME", state.path())
        .assert()
}

#[test]
fn a_variable_that_src_reads_and_the_spec_does_not_declare_is_a_failure_at_its_place() {
    starmap(&["check", "--spec", "undeclared/starmap.toml"])
        .code(1)
        .stdout_eq("undeclared/starmap.toml declares 1 variable, and undeclared/src reads it.\n")
        .stderr_eq(file!["golden/check-undeclared.stderr"]);
}

#[test]
fn the_json_counts_the_declared_reads_and_names_each_undeclared_one() {
    let run = starmap(&["check", "--spec", "undeclared/starmap.toml", "--json"]).code(1);
    let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    assert_eq!(document["data"]["read"], 1);
    assert_eq!(document["data"]["undeclared"], json!(["SHELF_THEME"]));
    assert_eq!(
        document["failures"][0]["code"],
        "starmap::variable::undeclared"
    );
}

#[test]
fn a_source_file_that_is_not_rust_fails_and_names_the_file() {
    starmap(&["check", "--spec", "unreadable/starmap.toml"])
        .code(1)
        .stdout_eq("")
        .stderr_eq(file!["golden/check-unreadable-source.stderr"]);
}
