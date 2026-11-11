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
fn a_read_of_a_member_is_declared_and_a_read_of_an_unlisted_key_is_undeclared() {
    let run = starmap(&["check", "--spec", "families/starmap.toml", "--json"]).code(1);
    let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    assert_eq!(
        document["data"]["undeclared"],
        json!(["SHELF_TOKEN_INITECH"])
    );
    assert_eq!(document["data"]["read"], 1);
}

#[test]
fn a_clap_env_attribute_is_a_read_that_passes_when_declared_and_fails_when_not() {
    starmap(&["check", "--spec", "clap/starmap.toml"])
        .code(1)
        .stdout_eq("clap/starmap.toml declares 1 variable, and clap/src reads it.\n")
        .stderr_eq(file!["golden/check-clap.stderr"]);
    let run = starmap(&["check", "--spec", "clap/starmap.toml", "--json"]).code(1);
    let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    assert_eq!(document["data"]["read"], 1);
    assert_eq!(
        document["data"]["undeclared"],
        json!(["SHELF_HOST", "SHELF_THEME"])
    );
}

#[test]
fn a_name_built_from_an_undeclared_prefix_fails_and_an_unresolved_name_is_a_second_sentence() {
    starmap(&["check", "--spec", "undeclared-family/starmap.toml"])
        .code(1)
        .stdout_eq(
            "undeclared-family/starmap.toml declares 1 variable, and undeclared-family/src reads it. \
             check cannot resolve the name read at undeclared-family/src/regions.rs:10.\n",
        )
        .stderr_eq(file!["golden/check-undeclared-family.stderr"]);
}

#[test]
fn the_json_lists_each_unresolved_place_and_fails_on_the_undeclared_family() {
    let run = starmap(&[
        "check",
        "--spec",
        "undeclared-family/starmap.toml",
        "--json",
    ])
    .code(1);
    let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    assert_eq!(
        document["data"]["unresolved"],
        json!(["undeclared-family/src/regions.rs:10"])
    );
    assert_eq!(document["data"]["undeclared"], json!([]));
    assert_eq!(
        document["failures"][0]["code"],
        "starmap::family::undeclared"
    );
    assert_eq!(
        document["failures"][0]["fields"]["prefix"],
        "SHELF_CACHE_URL"
    );
    assert_eq!(
        document["failures"][0]["fields"]["at"],
        "undeclared-family/src/regions.rs:9"
    );
    assert_eq!(document["failures"].as_array().unwrap().len(), 1);
}

#[test]
fn a_source_file_that_is_not_rust_fails_and_names_the_file() {
    starmap(&["check", "--spec", "unreadable/starmap.toml"])
        .code(1)
        .stdout_eq("")
        .stderr_eq(file!["golden/check-unreadable-source.stderr"]);
}

/// The spec whose clippy.toml disallows `std::env::var` and none of the
/// other methods that read the environment, relative to
/// [`SOURCE_FIXTURES`].
const UNBANNED: &str = "../../../starmap/tests/example/unbanned/starmap.toml";

#[test]
fn a_clippy_toml_that_lacks_an_entry_is_a_failure_that_names_each_method_and_init_with_clippy() {
    starmap(&["check", "--spec", UNBANNED])
        .code(1)
        .stderr_eq(file!["golden/check-unbanned.stderr"]);
    let run = starmap(&["check", "--spec", UNBANNED, "--json"]).code(1);
    let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    let failure = &document["failures"][0];
    assert_eq!(failure["code"], "starmap::clippy::unbanned");
    assert_eq!(
        failure["fields"]["methods"],
        json!(["std::env::var_os", "std::env::vars", "std::env::vars_os"])
    );
    assert_eq!(
        failure["fields"]["file"],
        "../../../starmap/tests/example/unbanned/clippy.toml"
    );
}

#[test]
fn a_clippy_toml_whose_disallowed_methods_is_not_an_array_fails_and_names_the_file() {
    starmap(&["check", "--spec", "clippy-invalid/starmap.toml"])
        .code(1)
        .stdout_eq("")
        .stderr_eq(file!["golden/check-clippy-invalid.stderr"]);
}

#[test]
fn a_clippy_toml_that_cannot_be_read_fails_and_names_the_file() {
    starmap(&["check", "--spec", "clippy-unreadable/starmap.toml"])
        .code(1)
        .stdout_eq("")
        .stderr_eq(file!["golden/check-clippy-unreadable.stderr"]);
}

#[test]
fn a_check_in_code_without_a_sentence_and_a_sentence_without_a_check_are_notes() {
    starmap(&["check", "--spec", "checks/starmap.toml"])
        .code(0)
        .stdout_eq("checks/starmap.toml declares 2 variables, and checks/src reads all 2.\n")
        .stderr_eq(
            "Mode: local, because the spec has no [mode] table.\n\
             checks/src/main.rs:3 checks SHELF_PORT in code, and checks/starmap.toml states no check sentence for it.\n\
             checks/starmap.toml states a check sentence for SHELF_REGION, and no field in checks/src names a check for it.\n",
        );
}

#[test]
fn the_json_names_each_check_without_a_sentence_and_each_sentence_without_a_check() {
    let run = starmap(&["check", "--spec", "checks/starmap.toml", "--json"]).code(0);
    let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    assert_eq!(
        document["data"]["checks_without_sentence"],
        json!(["SHELF_PORT"])
    );
    assert_eq!(
        document["data"]["sentences_without_check"],
        json!(["SHELF_REGION"])
    );
}
