//! Tests of the exit status of the built `starmap` binary.

#![cfg(test)]

use std::process::Command;

#[test]
fn the_starmap_binary_exits_with_success_when_run_without_arguments() {
    #[expect(
        clippy::disallowed_methods,
        reason = "the test runs the built binary, which is the behaviour it checks"
    )]
    let status = Command::new(env!("CARGO_BIN_EXE_starmap"))
        .status()
        .unwrap();
    assert!(status.success(), "starmap exited with {status}");
}
