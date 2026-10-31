//! End-to-end tests of how `starmap explain` shows a withheld value. Each
//! test runs the built binary on the build plan's illustrative
//! `starmap.toml` with a cleared environment.

#![cfg(test)]

use std::path::Path;

use serde_json::{Value, json};
use snapbox::cmd::{Command, OutputAssert};

/// The directory that holds the build plan's illustrative `starmap.toml`,
/// relative to this package.
const EXAMPLE: &str = "../starmap/tests/example";

/// The token that the tests set, which no output may hold.
const TOKEN: &str = "a-long-token-that-explain-must-never-print-0123456789";

/// Runs the built `starmap` binary with `args` in [`EXAMPLE`], with only
/// the variables `env` and a state directory under `state`.
fn starmap(state: &Path, args: &[&str], env: &[(&str, &str)]) -> OutputAssert {
    let mut command = Command::new(env!("CARGO_BIN_EXE_starmap"))
        .args(args)
        .current_dir(EXAMPLE)
        .env_clear()
        .env("XDG_STATE_HOME", state);
    for (name, value) in env {
        command = command.env(name, value);
    }
    command.assert()
}

/// Returns everything `run` wrote, standard output then standard error.
fn printed(run: &OutputAssert) -> String {
    let output = run.get_output();
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn a_withheld_value_prints_as_eight_dots_whatever_its_length() {
    let state = tempfile::tempdir().unwrap();
    for token in ["Z", TOKEN] {
        let run = starmap(
            state.path(),
            &["explain", "SHELF_MIRROR_TOKEN"],
            &[("SHELF_MIRROR_TOKEN", token)],
        )
        .success();
        let text = printed(&run);
        let row = text
            .lines()
            .find(|line| line.starts_with("SHELF_MIRROR_TOKEN"))
            .unwrap();
        assert!(row.ends_with("  ••••••••"), "{row}");
        assert!(!text.contains(token), "{text}");
    }
}

#[test]
fn a_withheld_value_in_json_has_its_state_and_no_text() {
    let state = tempfile::tempdir().unwrap();
    let run = starmap(
        state.path(),
        &["explain", "SHELF_MIRROR_TOKEN", "--json", "--full"],
        &[("SHELF_MIRROR_TOKEN", TOKEN)],
    )
    .success();
    assert!(!printed(&run).contains(TOKEN));
    let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    assert_eq!(document["data"][0]["value"], json!({"state": "set"}));
}

#[test]
fn an_unset_withheld_value_shows_that_it_is_unset() {
    let state = tempfile::tempdir().unwrap();
    let run = starmap(
        state.path(),
        &["explain", "SHELF_MIRROR_TOKEN", "--json"],
        &[],
    )
    .success();
    let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    assert_eq!(document["data"][0]["value"], json!({"state": "unset"}));
}

#[test]
fn a_shown_value_that_does_not_fit_its_type_is_shown_as_invalid() {
    let state = tempfile::tempdir().unwrap();
    let run = starmap(
        state.path(),
        &["explain", "SHELF_PORT", "--json"],
        &[("SHELF_PORT", "eighty")],
    )
    .success();
    let document: Value = serde_json::from_slice(&run.get_output().stdout).unwrap();
    assert_eq!(
        document["data"][0]["value"],
        json!({"state": "invalid", "text": "eighty"})
    );
}

#[test]
fn the_mode_note_names_the_production_value_that_decides_it() {
    let state = tempfile::tempdir().unwrap();
    starmap(state.path(), &["explain"], &[("SHELF_ENV", "prod")])
        .success()
        .stderr_eq(
            "Mode: production, because SHELF_ENV is prod. Values come from this shell, and explain resolves no bind.\n",
        );
}
