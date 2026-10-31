//! End-to-end tests of `starmap init`, and of `starmap check` on the file
//! that init writes. Each test copies the example application's src into a
//! checkout of its own, without a starmap.toml, and runs the built binary
//! there with a cleared environment.

#![cfg(test)]

use std::fs;
use std::path::Path;

use serde_json::Value;
use snapbox::cmd::{Command, OutputAssert};
use snapbox::file;
use tempfile::TempDir;

/// The example application's source, relative to this package.
const EXAMPLE_SRC: &str = "../starmap/tests/example/src";

/// Copies every file and directory under `from` to `to`.
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Returns a checkout that holds a copy of the example application's src
/// and no spec.
fn checkout() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(Path::new(EXAMPLE_SRC), &dir.path().join("src"));
    dir
}

/// Runs the built `starmap` binary with `args` in `dir`, with a cleared
/// environment and its usage log under `dir`'s state directory.
fn starmap(dir: &Path, args: &[&str]) -> OutputAssert {
    Command::new(env!("CARGO_BIN_EXE_starmap"))
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("XDG_STATE_HOME", dir.join("state"))
        .assert()
}

#[test]
fn init_writes_a_guess_for_each_variable_that_src_reads() {
    let dir = checkout();
    starmap(dir.path(), &["init"])
        .success()
        .stdout_eq(file!["golden/init/starmap-init.txt"])
        .stderr_eq(file!["golden/init/starmap-init.stderr"]);
    let written = fs::read_to_string(dir.path().join("starmap.toml")).unwrap();
    snapbox::assert_data_eq!(written, file!["golden/init/starmap.toml"]);
}

#[test]
fn init_prints_each_guess_and_its_places_as_json() {
    let dir = checkout();
    starmap(dir.path(), &["init", "--json"])
        .success()
        .stdout_eq(file!["golden/init/starmap-init-json.txt"]);
}

#[test]
fn a_dry_run_prints_the_guesses_and_writes_nothing() {
    let dir = checkout();
    starmap(dir.path(), &["init", "--dry-run"])
        .success()
        .stdout_eq(file!["golden/init/starmap-init.txt"])
        .stderr_eq(file!["golden/init/starmap-init-dry-run.stderr"]);
    assert!(!dir.path().join("starmap.toml").exists());
}

#[test]
fn check_refuses_every_guess_in_the_file_that_init_wrote() {
    let dir = checkout();
    starmap(dir.path(), &["init"]).success();
    let run = starmap(dir.path(), &["check", "--json"]).code(1);
    let failure: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    assert_eq!(failure["code"], "starmap::spec::guess");
    // Nine variables have five guesses each: purpose, type, unset, show and
    // source.
    let problems = failure["fields"]["problems"].as_array().unwrap();
    assert_eq!(problems.len(), 45);
    assert!(problems.iter().all(|kind| kind == "guess"));
}

#[test]
fn init_keeps_a_spec_that_exists_and_writes_nothing() {
    let dir = checkout();
    fs::write(dir.path().join("starmap.toml"), "# mine\n").unwrap();
    starmap(dir.path(), &["init"])
        .code(1)
        .stdout_eq("")
        .stderr_eq(file!["golden/init/starmap-init-exists.stderr"]);
    assert_eq!(
        fs::read_to_string(dir.path().join("starmap.toml")).unwrap(),
        "# mine\n"
    );
}
