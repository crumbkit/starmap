//! Tests of the command that `op_read` starts. The tests in
//! `tests/one_password.rs` run it against fake `op` programs.

use std::ffi::OsStr;
use std::path::Path;

use super::command;

#[test]
fn the_command_runs_the_given_program_with_the_reference_after_the_end_of_options() {
    let command = command(Path::new("/opt/op"), "op://Personal/shelf mirror/token");
    assert_eq!(command.get_program(), OsStr::new("/opt/op"));
    let args: Vec<&OsStr> = command.get_args().collect();
    assert_eq!(
        args,
        [
            "read",
            "--no-newline",
            "--",
            "op://Personal/shelf mirror/token"
        ]
    );
}

#[test]
fn the_command_sets_and_removes_no_variable_of_the_inherited_environment() {
    let command = command(Path::new("/opt/op"), "op://Personal/shelf mirror/token");
    assert_eq!(command.get_envs().count(), 0);
    assert_eq!(command.get_current_dir(), None);
}
