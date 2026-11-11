//! End-to-end tests of a `starmap.toml` that points to the file that
//! declares the variables. Each test runs the built binary in a checkout
//! of its own with a cleared environment and a state directory of its own.

#![cfg(test)]

use std::fs;
use std::path::Path;

use serde_json::Value;
use snapbox::cmd::{Command, OutputAssert};
use tempfile::TempDir;

/// The spec of the `run` fixtures, which each checkout keeps under
/// `config/environment.toml`.
const SPEC: &str = include_str!("fixtures/run/starmap.toml");

/// The pointer that each checkout keeps as its `starmap.toml`.
const POINTER: &str =
    "spec = \"config/environment.toml\"\nsrc = [\"app\"]\ndotenv = \"config/.env\"\n";

/// The script that prints the two settings of the spec.
const PRINT: &str = r#"printf '%s %s' "$SHELF_PAGE_SIZE" "$SHELF_DEBUG""#;

/// Runs the built `starmap` binary with `args` in `dir`, with a cleared
/// environment and its usage log under `dir`'s state directory.
fn starmap(dir: &Path, args: &[&str]) -> OutputAssert {
    Command::new(env!("CARGO_BIN_EXE_starmap"))
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("PATH", "/usr/bin:/bin")
        .assert()
}

/// Writes `text` to `path` under `dir`, creating its directories.
fn write(dir: &Path, path: &str, text: &str) {
    let path = dir.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// The clippy.toml that each checkout keeps, which disallows every method
/// that reads the environment.
const CLIPPY: &str = include_str!("../../starmap/tests/example/clippy.toml");

/// Returns a checkout whose `starmap.toml` is [`POINTER`], whose
/// declarations are in `config/environment.toml`, whose source in `app`
/// reads both settings, and whose `clippy.toml` is [`CLIPPY`].
fn checkout() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "starmap.toml", POINTER);
    write(dir.path(), "clippy.toml", CLIPPY);
    write(dir.path(), "config/environment.toml", SPEC);
    write(
        dir.path(),
        "app/main.rs",
        "fn main() {\n    let _ = std::env::var(\"SHELF_PAGE_SIZE\");\n    let _ = std::env::var(\"SHELF_DEBUG\");\n}\n",
    );
    dir
}

/// Returns the code of the failure document in `output`, or of its first
/// failure when the output is a result document that lists failures.
fn code(output: &[u8]) -> String {
    let document: Value = serde_json::from_slice(output).unwrap();
    let code = document["code"].as_str();
    code.or_else(|| document["failures"][0]["code"].as_str())
        .unwrap()
        .to_owned()
}

#[test]
fn check_follows_the_pointer_and_scans_the_source_it_lists() {
    let dir = checkout();

    starmap(dir.path(), &["check"])
        .code(0)
        .stdout_eq("config/environment.toml declares 3 variables, and app reads 2 of them.\n");
}

#[test]
fn check_through_the_pointer_gives_the_digest_of_the_file_it_names() {
    let dir = checkout();
    let digest = |args: &[&str]| {
        let run = starmap(dir.path(), args).code(0);
        let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
        document["data"]["digest"].as_str().unwrap().to_owned()
    };

    assert_eq!(
        digest(&["check", "--json"]),
        digest(&["check", "--json", "--spec", "config/environment.toml"])
    );
}

#[test]
fn check_follows_a_pointer_with_a_tool_table_and_ignores_the_tool_tables_of_both_files() {
    let dir = checkout();
    let digest = |dir: &Path| {
        let run = starmap(dir, &["check", "--json"]).code(0);
        let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
        document["data"]["digest"].as_str().unwrap().to_owned()
    };
    let before = digest(dir.path());
    write(
        dir.path(),
        "starmap.toml",
        &format!("{POINTER}\n[tool.codegen]\nmodule = \"settings\"\n"),
    );
    write(
        dir.path(),
        "config/environment.toml",
        &format!("[tool.codegen]\nmodule = \"settings\"\n\n{SPEC}"),
    );

    starmap(dir.path(), &["check"])
        .code(0)
        .stdout_eq("config/environment.toml declares 3 variables, and app reads 2 of them.\n");
    assert_eq!(digest(dir.path()), before);
}

#[test]
fn check_fails_for_a_variable_that_the_listed_source_reads_and_the_spec_does_not_declare() {
    let dir = checkout();
    write(
        dir.path(),
        "app/theme.rs",
        "fn theme() {\n    let _ = std::env::var(\"SHELF_THEME\");\n}\n",
    );

    let run = starmap(dir.path(), &["check", "--json"]).code(1);
    assert_eq!(
        code(&run.get_output().stdout),
        "starmap::variable::undeclared"
    );
}

#[test]
fn check_through_a_pointer_with_an_empty_src_scans_nothing() {
    let dir = checkout();
    write(
        dir.path(),
        "starmap.toml",
        "spec = \"config/environment.toml\"\nsrc = []\n",
    );
    // No source is scanned, so check asks for no clippy.toml.
    fs::remove_file(dir.path().join("clippy.toml")).unwrap();

    starmap(dir.path(), &["check"]).code(0).stdout_eq(
        "config/environment.toml declares 3 variables, and no source directory is scanned.\n",
    );
}

#[test]
fn run_through_a_pointer_takes_the_pointer_directory_as_the_checkout() {
    let dir = checkout();
    write(
        dir.path(),
        "config/environment.toml",
        "[variables.SHELF_DATA]\npurpose = \"The data directory.\"\ntype = \"path\"\nunset = \"required\"\nshow = \"shown\"\nsource = \"setting\"\nbind = { path = \"{checkout}/data\" }\n",
    );
    let root = dir.path().canonicalize().unwrap();

    starmap(
        dir.path(),
        &[
            "run",
            "--no-input",
            "--",
            "sh",
            "-c",
            r#"printf '%s' "$SHELF_DATA""#,
        ],
    )
    .code(0)
    .stdout_eq(format!("{}/data", root.display()));
}

#[test]
fn run_follows_the_pointer_and_reads_the_dotenv_it_names() {
    let dir = checkout();
    write(dir.path(), "config/.env", "SHELF_PAGE_SIZE=30\n");
    write(dir.path(), ".env", "SHELF_PAGE_SIZE=99\n");

    starmap(dir.path(), &["run", "--no-input", "--", "sh", "-c", PRINT])
        .code(0)
        .stdout_eq("30 false");
}

#[test]
fn explain_follows_the_pointer() {
    let dir = checkout();

    let run = starmap(dir.path(), &["explain", "SHELF_DEBUG", "--json"]).code(0);
    let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    assert_eq!(document["data"][0]["name"], "SHELF_DEBUG");
}

#[test]
fn init_writes_the_file_that_the_pointer_names_from_the_source_it_lists() {
    let dir = checkout();
    fs::remove_file(dir.path().join("config/environment.toml")).unwrap();
    fs::remove_file(dir.path().join("clippy.toml")).unwrap();

    starmap(dir.path(), &["init"]).success();

    let written = fs::read_to_string(dir.path().join("config/environment.toml")).unwrap();
    assert!(written.contains("[variables.SHELF_PAGE_SIZE]"), "{written}");
    assert!(written.contains("[variables.SHELF_DEBUG]"), "{written}");
    // init writes clippy.toml beside the pointer, where Clippy reads it.
    assert_eq!(
        fs::read_to_string(dir.path().join("clippy.toml")).unwrap(),
        CLIPPY
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("starmap.toml")).unwrap(),
        POINTER
    );
}

#[test]
fn init_keeps_the_file_that_the_pointer_names_when_it_exists() {
    let dir = checkout();

    starmap(dir.path(), &["init"]).code(1).stderr_eq(
        "error: config/environment.toml, which starmap.toml points to, exists already\n  help:  run `starmap check`\n  code:  starmap::spec::exists\n",
    );
}

#[test]
fn a_missing_file_that_the_pointer_names_fails_and_recovers_with_init() {
    let dir = checkout();
    fs::remove_file(dir.path().join("config/environment.toml")).unwrap();

    starmap(dir.path(), &["check"]).code(1).stderr_eq(
        "error: config/environment.toml, which starmap.toml points to, does not exist\n  help:  run `starmap init`\n  code:  starmap::spec::not_found\n",
    );
}

#[test]
fn a_pointer_that_declares_variables_is_refused() {
    let dir = checkout();
    write(dir.path(), "starmap.toml", &format!("{POINTER}\n{SPEC}"));

    let run = starmap(dir.path(), &["check", "--json"]).code(1);
    assert_eq!(
        code(&run.get_output().stdout),
        "starmap::spec::pointer_declares"
    );
}

#[test]
fn a_pointer_to_another_pointer_is_refused() {
    let dir = checkout();
    write(
        dir.path(),
        "config/environment.toml",
        "spec = \"shelf.toml\"\n",
    );

    let run = starmap(dir.path(), &["check", "--json"]).code(1);
    assert_eq!(
        code(&run.get_output().stdout),
        "starmap::spec::pointer_chain"
    );
}
