//! End-to-end tests of the command lines that a UI client runs. A UI client
//! adds `--json --full --no-input` to every command line it runs, so each
//! example in the guide must accept those flags and print a JSON document.

#![cfg(test)]

use clap::CommandFactory as _;
use serde_json::Value;
use snapbox::cmd::Command;
use starmap_cli::{Cli, GUIDE};

/// The directory that holds the example `starmap.toml`,
/// relative to this package.
const EXAMPLE: &str = "../starmap/tests/example";

/// The flags that a UI client adds to every command line it runs.
const UI_FLAGS: &[&str] = &["--json", "--full", "--no-input"];

/// Runs the built `starmap` binary with `words` and each of [`UI_FLAGS`]
/// that `words` lacks in [`EXAMPLE`], with a cleared environment and its
/// usage log in a temporary directory, and returns the JSON document that
/// it prints on standard output.
fn document(words: &[&str]) -> Value {
    let state = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_starmap"))
        .args(words)
        .args(UI_FLAGS.iter().filter(|flag| !words.contains(flag)))
        .current_dir(EXAMPLE)
        .env_clear()
        .env("XDG_STATE_HOME", state.path())
        .output()
        .unwrap();
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "starmap {} {} printed no JSON document: {error}\n{}",
            words.join(" "),
            UI_FLAGS.join(" "),
            String::from_utf8_lossy(&output.stderr),
        )
    })
}

/// Asserts that the command line parser accepts `words` with each of
/// [`UI_FLAGS`] that `words` lacks. The parser runs in process, so the
/// example's command, such as `init`, never runs and writes nothing.
fn assert_accepts_ui_flags(words: &[&str]) {
    let line: Vec<&str> = ["starmap"]
        .into_iter()
        .chain(words.iter().copied())
        .chain(
            UI_FLAGS
                .iter()
                .copied()
                .filter(|flag| !words.contains(flag)),
        )
        .collect();
    if let Err(error) = Cli::command().try_get_matches_from(&line) {
        panic!("{} refused the flags: {error}", line.join(" "));
    }
}

#[test]
fn every_example_in_the_guide_accepts_the_flags_that_a_ui_client_adds() {
    for leaf in GUIDE.get_leaves() {
        for example in leaf.get_examples() {
            let words: Vec<&str> = leaf.path().iter().chain(example.words()).copied().collect();
            assert_accepts_ui_flags(&words);
        }
    }
}

#[test]
fn the_command_list_with_all_prints_its_document_under_the_flags_that_a_ui_client_adds() {
    let document = document(&["commands", "list", "--all"]);
    assert_eq!(document["schema"], "starmap/commands-list/v1");
}

#[test]
fn the_schema_list_names_starmaps_own_documents() {
    let document = document(&["schemas", "list"]);
    assert_eq!(document["schema"], "starmap/schemas-list/v1");
    let ids: Vec<&str> = document["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    for id in [
        "starmap/failure/v1",
        "starmap/init/v1",
        "starmap/check/v1",
        "starmap/explain/v1",
    ] {
        assert!(ids.contains(&id), "schemas list misses {id}: {ids:?}");
    }
}

#[test]
fn schemas_show_prints_the_json_schema_of_the_failure_document() {
    let document = document(&["schemas", "show", "starmap/failure/v1"]);
    assert_eq!(document["schema"], "starmap/schemas-show/v1");
    assert_eq!(document["data"][0]["id"], "starmap/failure/v1");
    assert!(document["data"][0]["schema"].is_object());
}
