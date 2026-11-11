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

/// The example application's spec, relative to this package.
const EXAMPLE_SPEC: &str = "../starmap/tests/example/starmap.toml";

/// A clippy.toml that disallows `std::env::var` and none of the other
/// methods that read the environment, relative to this package.
const UNBANNED: &str = "../starmap/tests/example/unbanned/clippy.toml";

/// The words that name the entries that init writes to a new clippy.toml.
const ENTRIES: &str = "disallowed-methods entries for std::env::var, std::env::var_os, std::env::vars and std::env::vars_os";

/// The sentence that init prints when it read no `.rs` file.
const NO_RUST: &str = "init read no .rs file, so it wrote no clippy.toml.";

/// The column header that init prints, alone when it lists no variable.
const HEADER: &str = "NAME  TYPE  UNSET  SOURCE  READ_AT\n";

/// The note that init prints when it lists no variable.
const NO_VARIABLES: &str = "note: init lists no variables\n";

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
    assert!(!dir.path().join("clippy.toml").exists());
}

#[test]
fn init_writes_a_clippy_toml_that_disallows_each_method_that_reads_the_environment() {
    let dir = checkout();
    starmap(dir.path(), &["init"]).success();
    let written = fs::read_to_string(dir.path().join("clippy.toml")).unwrap();
    snapbox::assert_data_eq!(written, file!["../../starmap/tests/example/clippy.toml"]);
}

#[test]
fn init_adds_the_missing_entries_to_a_clippy_toml_and_keeps_its_entries_and_comments() {
    let dir = checkout();
    fs::copy(UNBANNED, dir.path().join("clippy.toml")).unwrap();
    starmap(dir.path(), &["init"])
        .success()
        .stderr_eq(file!["golden/init/starmap-init-clippy-added.stderr"]);
    let written = fs::read_to_string(dir.path().join("clippy.toml")).unwrap();
    snapbox::assert_data_eq!(written, file!["golden/init/clippy-added.toml"]);
}

#[test]
fn init_with_clippy_adds_the_entries_beside_a_spec_that_exists_and_keeps_the_spec() {
    let dir = checkout();
    let spec = fs::read_to_string(EXAMPLE_SPEC).unwrap();
    fs::write(dir.path().join("starmap.toml"), &spec).unwrap();
    let run = starmap(dir.path(), &["check", "--json"]).code(1);
    let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    assert_eq!(document["failures"][0]["code"], "starmap::clippy::unbanned");

    starmap(dir.path(), &["init", "--clippy", "--dry-run"])
        .success()
        .stdout_eq(HEADER)
        .stderr_eq(format!(
            "{NO_VARIABLES}init would write clippy.toml with {ENTRIES}.\n\
             This was a dry run, so nothing changed.\n"
        ));
    assert!(!dir.path().join("clippy.toml").exists());

    starmap(dir.path(), &["init", "--clippy"])
        .success()
        .stdout_eq(HEADER)
        .stderr_eq(format!("{NO_VARIABLES}Wrote clippy.toml with {ENTRIES}.\n"));
    // The JSON is the document of init, with no variable.
    let run = starmap(dir.path(), &["init", "--clippy", "--json"]).success();
    let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    assert_eq!(document["schema"], "starmap/init/v1");
    assert_eq!(document["data"], serde_json::json!([]));
    assert_eq!(document["total"], 0);
    let written = fs::read_to_string(dir.path().join("clippy.toml")).unwrap();
    snapbox::assert_data_eq!(written, file!["../../starmap/tests/example/clippy.toml"]);
    assert_eq!(
        fs::read_to_string(dir.path().join("starmap.toml")).unwrap(),
        spec
    );
    starmap(dir.path(), &["check"]).success();
}

#[test]
fn init_without_rust_source_lists_no_variables_and_writes_no_clippy_toml() {
    let dir = tempfile::tempdir().unwrap();
    starmap(dir.path(), &["init"])
        .success()
        .stdout_eq(HEADER)
        .stderr_eq(format!(
            "{NO_VARIABLES}\
             Wrote starmap.toml with 0 variables that src reads. Each value marked ? is a guess.\n\
             {NO_RUST}\n\
             Next: starmap check refuses the file until each guess is confirmed and its ? deleted.\n"
        ));
    starmap(dir.path(), &["init", "--clippy"])
        .success()
        .stdout_eq(HEADER)
        .stderr_eq(format!("{NO_VARIABLES}{NO_RUST}\n"));
    assert!(!dir.path().join("clippy.toml").exists());
    starmap(dir.path(), &["check"]).success();
}

#[cfg(unix)]
#[test]
fn init_writes_through_a_clippy_toml_that_is_a_symbolic_link_and_keeps_the_link() {
    let dir = checkout();
    fs::create_dir(dir.path().join("shared")).unwrap();
    let target = dir.path().join("shared/clippy.toml");
    fs::copy(UNBANNED, &target).unwrap();
    let link = dir.path().join("clippy.toml");
    std::os::unix::fs::symlink("shared/clippy.toml", &link).unwrap();

    // init names the file that it changed, which the link leads to.
    let run = starmap(dir.path(), &["init"]).success();
    let stderr = String::from_utf8_lossy(&run.get_output().stderr).into_owned();
    assert!(
        stderr.contains("Added disallowed-methods entries to shared/clippy.toml for "),
        "{stderr}"
    );

    assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
    let written = fs::read_to_string(&target).unwrap();
    snapbox::assert_data_eq!(written, file!["golden/init/clippy-added.toml"]);
}

#[cfg(unix)]
#[test]
fn init_refuses_a_clippy_toml_that_is_a_symbolic_link_to_no_file_and_writes_nothing() {
    let dir = checkout();
    let link = dir.path().join("clippy.toml");
    std::os::unix::fs::symlink("missing.toml", &link).unwrap();

    let run = starmap(dir.path(), &["init", "--json"]).code(1);
    let failure: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    assert_eq!(failure["code"], "starmap::clippy::invalid");
    let message = failure["message"].as_str().unwrap();
    assert!(
        message.starts_with("clippy.toml cannot be read"),
        "{message}"
    );
    assert!(!dir.path().join("starmap.toml").exists());
    assert!(!dir.path().join("missing.toml").exists());
}

#[test]
fn init_leaves_a_clippy_toml_above_the_repository_as_it_is_and_writes_one_inside() {
    let above = tempfile::tempdir().unwrap();
    let lints = "too-many-lines-threshold = 150\n";
    fs::write(above.path().join("clippy.toml"), lints).unwrap();
    let repo = above.path().join("repo");
    copy_tree(Path::new(EXAMPLE_SRC), &repo.join("src"));
    fs::create_dir(repo.join(".git")).unwrap();
    fs::write(repo.join("Cargo.toml"), "[package]\nname = \"shelf\"\n").unwrap();

    starmap(&repo, &["init"]).success();

    assert_eq!(
        fs::read_to_string(above.path().join("clippy.toml")).unwrap(),
        lints
    );
    let written = fs::read_to_string(repo.join("clippy.toml")).unwrap();
    snapbox::assert_data_eq!(written, file!["../../starmap/tests/example/clippy.toml"]);
    starmap(&repo, &["check", "--json"]).code(1);
    starmap(&repo, &["init", "--clippy"]).success();
}

#[test]
fn init_that_cannot_write_clippy_toml_keeps_the_spec_and_names_init_with_clippy() {
    let dir = checkout();
    fs::copy(UNBANNED, dir.path().join("clippy.toml")).unwrap();
    // A directory stands where init writes the new text of clippy.toml.
    let blocker = dir.path().join("clippy.toml.starmap-new");
    fs::create_dir(&blocker).unwrap();

    starmap(dir.path(), &["init"])
        .code(1)
        .stderr_eq(file!["golden/init/starmap-init-clippy-unwritable.stderr"]);
    assert!(dir.path().join("starmap.toml").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("clippy.toml")).unwrap(),
        fs::read_to_string(UNBANNED).unwrap()
    );

    fs::remove_dir(&blocker).unwrap();
    starmap(dir.path(), &["init", "--clippy"]).success();
    let written = fs::read_to_string(dir.path().join("clippy.toml")).unwrap();
    snapbox::assert_data_eq!(written, file!["golden/init/clippy-added.toml"]);
}

#[test]
fn init_refuses_a_clippy_toml_whose_disallowed_methods_is_not_an_array_and_writes_nothing() {
    let dir = checkout();
    let text = "disallowed-methods = \"std::env::var\"\n";
    fs::write(dir.path().join("clippy.toml"), text).unwrap();
    starmap(dir.path(), &["init"])
        .code(1)
        .stdout_eq("")
        .stderr_eq(file!["golden/init/starmap-init-clippy-invalid.stderr"]);
    let run = starmap(dir.path(), &["init", "--clippy", "--json"]).code(1);
    let failure: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    assert_eq!(failure["code"], "starmap::clippy::invalid");
    assert!(!dir.path().join("starmap.toml").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("clippy.toml")).unwrap(),
        text
    );
}

#[test]
fn init_guesses_a_variable_for_each_clap_env_attribute() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/cli.rs"),
        "struct Cli {\n    #[arg(long, env = \"SHELF_PORT\")]\n    port: u16,\n    #[arg(long, env)]\n    shelf_host: String,\n}\n",
    )
    .unwrap();
    let run = starmap(dir.path(), &["init", "--dry-run", "--json"]).success();
    let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    let guesses: Vec<(&str, &str)> = document["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|guess| {
            (
                guess["name"].as_str().unwrap(),
                guess["read_at"][0].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        guesses,
        [
            ("SHELF_HOST", "src/cli.rs:4"),
            ("SHELF_PORT", "src/cli.rs:2")
        ]
    );
}

#[test]
fn check_refuses_every_guess_in_the_file_that_init_wrote() {
    let dir = checkout();
    starmap(dir.path(), &["init"]).success();
    starmap(dir.path(), &["check"])
        .code(1)
        .stdout_eq("")
        .stderr_eq(file!["golden/init/check-guesses.stderr"]);
    let run = starmap(dir.path(), &["check", "--json"]).code(1);
    let failure: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    assert_eq!(failure["code"], "starmap::spec::guess");
    // Nine variables have five guesses each: purpose, type, unset, show and
    // source.
    let problems = failure["fields"]["problems"].as_array().unwrap();
    assert_eq!(problems.len(), 45);
    assert!(problems.iter().all(|kind| kind == "starmap::spec::guess"));
}

#[test]
fn init_keeps_a_spec_that_exists_and_writes_nothing() {
    let dir = checkout();
    fs::write(dir.path().join("starmap.toml"), "# mine\n").unwrap();
    starmap(dir.path(), &["init"])
        .code(1)
        .stdout_eq("")
        .stderr_eq(file!["golden/init/starmap-init-exists.stderr"]);
    // A dry run refuses the existing file too, before it scans src.
    starmap(dir.path(), &["init", "--dry-run"])
        .code(1)
        .stdout_eq("")
        .stderr_eq(file!["golden/init/starmap-init-exists.stderr"]);
    assert_eq!(
        fs::read_to_string(dir.path().join("starmap.toml")).unwrap(),
        "# mine\n"
    );
}
