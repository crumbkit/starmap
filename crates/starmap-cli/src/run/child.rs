//! The child of `starmap run`. This module builds the program's command,
//! and crumbkit-child starts it (PROC-1).
//!
//! Each run states the five items of PROC-2.
//!
//! - Program: the first word after `--`, found on the `PATH` of the
//!   process that runs starmap. The person who runs starmap names it.
//! - Arguments: the remaining words after `--`, each a separate value. No
//!   shell parses them, and no value of a variable is ever an argument.
//! - Environment: the child inherits the environment of the process that
//!   runs starmap, and `Environment::apply` then sets each declared
//!   variable that has a value and removes each that stays unset. With
//!   `--only-declared`, the child inherits nothing:
//!   `Environment::apply_only_declared` clears the environment and sets
//!   the declared values and the built-in variables of
//!   `starmap::BuiltIns` that the process that runs starmap has. In both
//!   cases crumbkit-child adds `TRACEPARENT` and `TRACESTATE`, the
//!   variables that join the run's trace, when the run is recorded.
//! - Streams and deadline: the child reads and writes the terminal
//!   directly, and has no deadline, because it is the program the person
//!   asked to run.
//! - End: crumbkit-child waits for the child and reaps it. starmap ignores
//!   SIGINT and SIGQUIT from the moment the child starts, so Ctrl-C and
//!   Ctrl-\ reach only the child, and starmap returns the child's exit
//!   code, or 128 plus the number of the signal that ended it.

use std::ffi::OsString;
use std::process::Command;

use starmap::{BuiltIns, Environment};

/// Returns the command that runs `words`: the program and its arguments.
pub(super) fn command(words: &[OsString]) -> Command {
    let (program, args) = words.split_first().map_or_else(
        || (OsString::new(), &[][..]),
        |(program, args)| (program.clone(), args),
    );
    #[expect(
        clippy::disallowed_methods,
        reason = "crates/starmap-cli/src/run/child.rs builds the child of starmap run, which crumbkit-child starts"
    )]
    let mut command = Command::new(program);
    command.args(args);
    command
}

/// Sets the checked values of `environment` on `command`. With
/// `inherited`, the environment of the process that runs starmap, which a
/// run with `--only-declared` passes, the child receives only those values
/// and the built-in variables of this platform. Without it, the child
/// inherits every other variable.
pub(super) fn apply(
    environment: &Environment,
    inherited: Option<Vec<(OsString, OsString)>>,
    command: &mut Command,
) {
    match inherited {
        Some(inherited) => {
            environment.apply_only_declared(command, BuiltIns::current(), inherited);
        }
        None => environment.apply(command),
    }
}
