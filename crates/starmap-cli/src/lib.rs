//! The `starmap` command, which writes, checks, explains and runs an
//! application's declared environment.
//!
//! The library holds the whole command, and the `starmap` binary calls
//! `run`. It exports the parser, the tool record and the failure codes, so the
//! integration tests can pass them to crumbkit-test's checks beside the
//! built binary. It is not an API for other crates, so each public item is
//! hidden from the documentation and outside the semver promise.

mod check;
mod cli;
mod clippy;
#[doc(hidden)]
pub mod codes;
mod diff;
mod dotenv;
mod explain;
mod files;
mod git;
mod init;
mod mode;
mod run;
mod scan;
mod spec;

use std::ffi::OsString;
use std::process::ExitCode;

use clap::CommandFactory as _;

#[doc(hidden)]
pub use crate::cli::{Cli, TOOL};
use crate::cli::{
    CommandsCommand, CompletionCommand, SKILL, SchemasCommand, SelfCommand, SkillsCommand, Verb,
};

/// Parses `args`, runs the command and returns the process's exit code. The
/// first argument is the program's name. `vars` is the process environment,
/// which `main` reads once (BASE-6), and `check`, `explain` and `run` read
/// the declared variables from it.
///
/// `crumbkit::parse_from` prints help or a refused command line and returns
/// the exit code. `crumbkit::finish` prints the result or the failure in
/// the format that the output flags choose, and returns the exit code.
#[doc(hidden)]
#[must_use]
pub fn run<I, T>(args: I, vars: Vec<(OsString, OsString)>) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let (cli, mut output) = match crumbkit::parse_from::<Cli>(&TOOL, args) {
        Ok(parsed) => parsed,
        Err(stop) => return stop.exit_code(),
    };
    match cli.verb {
        Verb::Init(args) => {
            let dry_run = output.is_dry_run();
            if args.clippy {
                crumbkit::finish(output, init::BAN, init::ban(&args, dry_run))
            } else {
                crumbkit::finish(output, init::INIT, init::init(&args, dry_run))
            }
        }
        Verb::Check(args) => crumbkit::finish(output, check::CHECK, check::check(&args, vars)),
        Verb::Explain(args) => {
            crumbkit::finish(output, explain::EXPLAIN, explain::explain(&args, vars))
        }
        Verb::Diff(args) => crumbkit::finish(output, diff::DIFF, diff::diff(&args)),
        Verb::Run(args) => {
            #[expect(
                clippy::disallowed_methods,
                reason = "run prints its notes on standard error before the child takes the terminal, after which nothing of starmap prints"
            )]
            let mut notes = std::io::stderr();
            run::run(&args, vars, output, &mut notes)
        }
        // `commands list`, `schemas` and `self usage` read the command tree through
        // `&mut output`, so each runs before `finish` takes `output`.
        Verb::Commands {
            command: CommandsCommand::List(args),
        } => {
            let result = crumbkit::commands::list(&mut output, &TOOL, &args);
            crumbkit::finish(output, crumbkit::commands::COMMANDS_LIST, result)
        }
        Verb::Schemas {
            command: SchemasCommand::List(args),
        } => {
            let result = crumbkit::schemas::list(&mut output, &TOOL, &args);
            crumbkit::finish(output, crumbkit::schemas::SCHEMAS_LIST, result)
        }
        Verb::Schemas {
            command: SchemasCommand::Show(args),
        } => {
            let result = crumbkit::schemas::show(&mut output, &TOOL, &args);
            crumbkit::finish(output, crumbkit::schemas::SCHEMAS_SHOW, result)
        }
        Verb::SelfCommand {
            command: SelfCommand::Usage(args),
        } => {
            let result =
                Ok::<_, crumbkit::Error>(crumbkit_usage::findings(&mut output, &TOOL, &args));
            crumbkit::finish(output, crumbkit_usage::SELF_USAGE, result)
        }
        Verb::Skills { command } => match command {
            SkillsCommand::Install(args) => {
                let result = Ok::<_, crumbkit::Error>(crumbkit_skills::install(&SKILL, &args));
                crumbkit::finish(output, crumbkit_skills::SKILLS_INSTALL, result)
            }
            SkillsCommand::Check(args) => {
                let result = Ok::<_, crumbkit::Error>(crumbkit_skills::check(&SKILL, &args));
                crumbkit::finish(output, crumbkit_skills::SKILLS_CHECK, result)
            }
            SkillsCommand::Print => {
                let result = Ok::<_, crumbkit::Error>(crumbkit_skills::print(&SKILL));
                crumbkit::finish(output, crumbkit_skills::SKILLS_PRINT, result)
            }
        },
        Verb::Completion { command } => match command {
            CompletionCommand::Install(args) => {
                let result = crumbkit::completion::install(Cli::command, &args);
                crumbkit::finish(output, crumbkit::completion::COMPLETION_INSTALL, result)
            }
            CompletionCommand::Check(args) => {
                let result = crumbkit::completion::check(Cli::command, &args);
                crumbkit::finish(output, crumbkit::completion::COMPLETION_CHECK, result)
            }
            CompletionCommand::Print(args) => {
                let result = crumbkit::completion::print(Cli::command, &args);
                crumbkit::finish(output, crumbkit::completion::COMPLETION_PRINT, result)
            }
        },
    }
}
