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

    starmap(dir.path(), &["check"]).code(0).stdout_eq(
        "config/environment.toml declares 3 variables, and no source directory is scanned.\n",
    );
}

#[test]
fn check_names_the_clippy_toml_of_a_member_crate_that_lacks_the_entries_and_init_adds_them() {
    let dir = checkout();
    write(
        dir.path(),
        "starmap.toml",
        "spec = \"config/environment.toml\"\nsrc = [\"crates/a/src\", \"crates/b/src\"]\n",
    );
    let read = "fn size() {\n    let _ = std::env::var(\"SHELF_PAGE_SIZE\");\n}\n";
    write(dir.path(), "crates/a/src/lib.rs", read);
    write(dir.path(), "crates/b/src/lib.rs", read);
    write(dir.path(), "crates/a/Cargo.toml", "");
    write(dir.path(), "crates/b/Cargo.toml", "");
    // Clippy reads only this file for crate b, and the file at the root
    // for crate a.
    let lints = "too-many-lines-threshold = 150\n";
    write(dir.path(), "crates/b/clippy.toml", lints);

    let run = starmap(dir.path(), &["check", "--json"]).code(1);
    let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    let failures = document["failures"].as_array().unwrap();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0]["code"], "starmap::clippy::unbanned");
    assert_eq!(failures[0]["fields"]["file"], "crates/b/clippy.toml");

    starmap(dir.path(), &["init", "--clippy"])
        .success()
        .stdout_eq("NAME  TYPE  UNSET  SOURCE  READ_AT\n")
        .stderr_eq(
            "note: init lists no variables\n\
             clippy.toml already disallows std::env::var, std::env::var_os, std::env::vars and std::env::vars_os.\n\
             Added disallowed-methods entries to crates/b/clippy.toml for std::env::var, std::env::var_os, std::env::vars and std::env::vars_os.\n",
        );
    let written = fs::read_to_string(dir.path().join("crates/b/clippy.toml")).unwrap();
    assert!(written.starts_with(lints), "{written}");
    starmap(dir.path(), &["check"]).success();
}

/// Returns the file that each `starmap::clippy::unbanned` failure of
/// `check` in `dir` names.
fn unbanned(dir: &Path) -> Vec<String> {
    let run = starmap(dir, &["check", "--json"]);
    let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    let failures = document["failures"].as_array().cloned().unwrap_or_default();
    failures
        .iter()
        .filter(|failure| failure["code"] == "starmap::clippy::unbanned")
        .map(|failure| failure["fields"]["file"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn check_with_the_checkout_as_src_reads_the_clippy_toml_beside_its_cargo_toml() {
    let dir = checkout();
    write(
        dir.path(),
        "starmap.toml",
        "spec = \"config/environment.toml\"\nsrc = [\".\"]\n",
    );
    write(dir.path(), "Cargo.toml", "");

    starmap(dir.path(), &["check"]).success();
    write(dir.path(), "clippy.toml", "");
    assert_eq!(unbanned(dir.path()), ["clippy.toml"]);
}

#[test]
fn check_with_a_directory_of_crates_as_src_reads_the_clippy_toml_of_each_crate() {
    let dir = checkout();
    write(
        dir.path(),
        "starmap.toml",
        "spec = \"config/environment.toml\"\nsrc = [\"crates\"]\n",
    );
    let read = "fn size() {\n    let _ = std::env::var(\"SHELF_PAGE_SIZE\");\n}\n";
    for krate in ["a", "b"] {
        write(dir.path(), &format!("crates/{krate}/Cargo.toml"), "");
        write(dir.path(), &format!("crates/{krate}/src/lib.rs"), read);
        write(dir.path(), &format!("crates/{krate}/src/net/mod.rs"), read);
    }
    starmap(dir.path(), &["check"]).success();

    // Clippy reads only this file for crate b.
    write(dir.path(), "crates/b/clippy.toml", "");
    assert_eq!(unbanned(dir.path()), ["crates/b/clippy.toml"]);
    starmap(dir.path(), &["init", "--clippy"]).success();
    starmap(dir.path(), &["check"]).success();
}

#[test]
fn a_relative_spec_path_names_the_clippy_toml_beside_the_spec() {
    let dir = checkout();
    let pointer = fs::read_to_string(dir.path().join("starmap.toml")).unwrap();
    write(dir.path(), "shelf/starmap.toml", &pointer);
    write(dir.path(), "shelf/config/environment.toml", SPEC);
    write(dir.path(), "shelf/Cargo.toml", "");
    fs::rename(dir.path().join("app"), dir.path().join("shelf/app")).unwrap();
    // The checkout has no repository, so the file above the spec is not
    // read.
    let spec = ["--spec", "shelf/starmap.toml"];

    let run = starmap(dir.path(), &[&["check", "--json"], &spec[..]].concat()).code(1);
    let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    assert_eq!(
        document["failures"][0]["fields"]["file"],
        "shelf/clippy.toml"
    );
    starmap(dir.path(), &[&["init", "--clippy"], &spec[..]].concat()).success();
    assert_eq!(
        fs::read_to_string(dir.path().join("shelf/clippy.toml")).unwrap(),
        CLIPPY
    );
    starmap(dir.path(), &[&["check"], &spec[..]].concat()).success();
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
