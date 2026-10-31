//! Tests of the command that `op_read` starts. The tests in
//! `tests/one_password.rs` run it against fake `op` programs and check its
//! arguments.

use std::path::Path;

use super::command;

#[test]
fn the_command_sets_and_removes_no_variable_of_the_inherited_environment() {
    let command = command(Path::new("/opt/op"), "op://Personal/shelf mirror/token");
    assert_eq!(command.get_envs().count(), 0);
}

#[test]
fn the_command_gives_op_an_empty_standard_input() {
    let command = command(Path::new("/opt/op"), "op://Personal/shelf mirror/token");
    let shown = format!("{command:#?}");
    let compact: String = shown.split_whitespace().collect();
    assert!(compact.contains("stdin:Some(Null,)"), "{shown}");
}
