//! The `starmap` command, which checks and explains an application's
//! declared environment.
//!
//! The library holds the whole command, and the `starmap` binary calls
//! [`run`]. It exports the parser, the guide and the failure codes, so the
//! integration tests can pass them to crumbkit-test's checks beside the
//! built binary. It is not an API for other crates.

mod check;
mod cli;
pub mod codes;
mod explain;
mod files;
mod mode;
mod spec;

use std::ffi::OsString;
use std::process::ExitCode;

use clap::CommandFactory as _;

pub use crate::cli::{Cli, GUIDE};
use crate::cli::{CommandsCommand, SelfCommand, Verb};

/// Every failure code that the `starmap` command declares, which the tests
/// pass to crumbkit-test's codes, recovery and twin checks.
pub const CODES: &[&crumbkit::Code] = codes::ALL;

/// Parses `args`, runs the command and returns the process's exit code. The
/// first argument is the program's name. `vars` is the process environment,
/// which `main` reads once (BASE-6), and `check` and `explain` read the
/// declared variables from it.
///
/// `crumbkit::parse_from` prints help or a refused command line and returns
/// the exit code. `crumbkit::finish` prints the result or the failure in
/// the format that the output flags choose, and returns the exit code.
#[must_use]
pub fn run<I, T>(args: I, vars: Vec<(OsString, OsString)>) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let (cli, mut output) = match crumbkit::parse_from::<Cli, _, _>(Cli::command(), &GUIDE, args) {
        Ok(parsed) => parsed,
        Err(stop) => return stop.exit_code(),
    };
    match cli.verb {
        Verb::Check(args) => crumbkit::finish(output, check::CHECK, check::check(&args, vars)),
        Verb::Explain(args) => {
            crumbkit::finish(output, explain::EXPLAIN, explain::explain(&args, vars))
        }
        // `commands list` and `self usage` read the command tree through
        // `&mut output`, so each runs before `finish` takes `output`.
        Verb::Commands {
            command: CommandsCommand::List(args),
        } => {
            let result = crumbkit::commands::list(&mut output, &GUIDE, &args);
            crumbkit::finish(output, crumbkit::commands::COMMANDS_LIST, result)
        }
        Verb::SelfCommand {
            command: SelfCommand::Usage(args),
        } => {
            let result =
                Ok::<_, crumbkit::Error>(crumbkit_usage::findings(&mut output, &GUIDE, &args));
            crumbkit::finish(output, crumbkit_usage::SELF_USAGE, result)
        }
    }
}
