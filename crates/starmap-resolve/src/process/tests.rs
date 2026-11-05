//! Tests of the commands that `op_read` and `op_inject` start. The tests in
//! `tests/one_password.rs` run them against fake `op` programs and check
//! their arguments.

use std::path::Path;

use super::{inject_command, read_command};

#[test]
fn the_command_sets_and_removes_no_variable_of_the_inherited_environment() {
    let command = read_command(Path::new("/opt/op"), "op://Personal/shelf mirror/token");
    assert_eq!(command.get_envs().count(), 0);
}

#[test]
fn the_command_gives_op_an_empty_standard_input() {
    let command = read_command(Path::new("/opt/op"), "op://Personal/shelf mirror/token");
    let shown = format!("{command:#?}");
    let compact: String = shown.split_whitespace().collect();
    assert!(compact.contains("stdin:Some(Null,)"), "{shown}");
}

#[test]
fn the_inject_command_takes_no_reference_as_an_argument_and_sets_no_variable() {
    let command = inject_command(Path::new("/opt/op"));
    let args: Vec<_> = command.get_args().collect();
    assert_eq!(args, ["inject"]);
    assert_eq!(command.get_envs().count(), 0);
}

#[test]
fn the_inject_command_reads_its_template_from_a_pipe() {
    let command = inject_command(Path::new("/opt/op"));
    let shown = format!("{command:#?}");
    let compact: String = shown.split_whitespace().collect();
    assert!(compact.contains("stdin:Some(MakePipe,)"), "{shown}");
}
