//! End-to-end tests of the `starmap` command's usage log (CLI-7). Each test
//! runs the built binary with a state directory of its own and reads the
//! lines the runs appended to `starmap/usage.jsonl` there.

#![cfg(test)]

use std::fs;

use serde_json::Value;
use snapbox::cmd::Command;

#[test]
fn each_run_appends_one_line_that_names_its_command_and_exit_code() {
    let state = tempfile::tempdir().unwrap();
    for args in [&["check"][..], &["explain", "SHELF_THEME"][..]] {
        Command::new(env!("CARGO_BIN_EXE_starmap"))
            .args(args)
            .current_dir("../starmap/tests/example")
            .env_clear()
            .env("XDG_STATE_HOME", state.path())
            .output()
            .unwrap();
    }
    let log = fs::read_to_string(state.path().join("starmap").join("usage.jsonl")).unwrap();
    let lines: Vec<Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let commands: Vec<(&str, u64)> = lines
        .iter()
        .map(|line| {
            (
                line["command"].as_str().unwrap(),
                line["exit"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(commands, [("check", 0), ("explain", 1)]);
}
