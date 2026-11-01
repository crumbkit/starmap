//! The `starmap` command, which writes, checks and explains an
//! application's declared environment.
//!
//! The library holds the whole command, and the `starmap` binary calls
//! [`run`]. It exports the parser, the guide and the failure codes, so the
//! integration tests can pass them to crumbkit-test's checks beside the
//! built binary. It is not an API for other crates, so each public item is
//! hidden from the documentation and outside the semver promise.

mod check;
mod cli;
#[doc(hidden)]
pub mod codes;
mod diff;
mod explain;
mod files;
mod git;
mod init;
mod mode;
mod scan;
mod spec;

use std::ffi::OsString;
use std::fmt;
use std::process::ExitCode;

use clap::CommandFactory as _;
use crumbkit::Failure;

#[doc(hidden)]
pub use crate::cli::{Cli, GUIDE};
use crate::cli::{CommandsCommand, SchemasCommand, SelfCommand, Verb};
use crate::diff::RevisionFailure;
use crate::scan::SourceFailure;
use crate::spec::SpecFailure;

/// Parses `args`, runs the command and returns the process's exit code. The
/// first argument is the program's name. `vars` is the process environment,
/// which `main` reads once (BASE-6), and `check` and `explain` read the
/// declared variables from it.
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
    let (cli, mut output) = match crumbkit::parse_from::<Cli, _, _>(Cli::command(), &GUIDE, args) {
        Ok(parsed) => parsed,
        Err(stop) => return stop.exit_code(),
    };
    match cli.verb {
        Verb::Init(args) => {
            let dry_run = output.is_dry_run();
            crumbkit::finish(output, init::INIT, init::init(&args, dry_run))
        }
        Verb::Check(args) => crumbkit::finish(output, check::CHECK, check::check(&args, vars)),
        Verb::Explain(args) => {
            crumbkit::finish(output, explain::EXPLAIN, explain::explain(&args, vars))
        }
        Verb::Diff(args) => crumbkit::finish(output, diff::DIFF, diff::diff(&args)),
        // `commands list`, `schemas` and `self usage` read the command tree through
        // `&mut output`, so each runs before `finish` takes `output`.
        Verb::Commands {
            command: CommandsCommand::List(args),
        } => {
            let result = crumbkit::commands::list(&mut output, &GUIDE, &args);
            crumbkit::finish(output, crumbkit::commands::COMMANDS_LIST, result)
        }
        Verb::Schemas {
            command: SchemasCommand::List(args),
        } => {
            let result = crumbkit::schemas::list(&mut output, &GUIDE, &args);
            crumbkit::finish(output, crumbkit::schemas::SCHEMAS_LIST, result)
        }
        Verb::Schemas {
            command: SchemasCommand::Show(args),
        } => {
            let result = crumbkit::schemas::show(&mut output, &GUIDE, &args);
            crumbkit::finish(output, crumbkit::schemas::SCHEMAS_SHOW, result)
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

/// A failure of `init`, `check` or `diff`, which read or write the spec,
/// scan the source or read a git revision.
#[derive(Debug, Failure)]
pub(crate) enum Failed {
    /// The spec cannot be read, parsed or written.
    #[failure(forward, diagnostic)]
    Spec(SpecFailure),
    /// A file under src cannot be read.
    #[failure(forward)]
    Source(SourceFailure),
    /// git cannot read the revision that `diff` names.
    #[failure(forward)]
    Revision(RevisionFailure),
}

impl fmt::Display for Failed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spec(failure) => failure.fmt(formatter),
            Self::Source(failure) => failure.fmt(formatter),
            Self::Revision(failure) => failure.fmt(formatter),
        }
    }
}

impl std::error::Error for Failed {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spec(failure) => failure.source(),
            Self::Source(failure) => failure.source(),
            Self::Revision(failure) => failure.source(),
        }
    }
}

impl From<SpecFailure> for Failed {
    fn from(failure: SpecFailure) -> Self {
        Self::Spec(failure)
    }
}

impl From<SourceFailure> for Failed {
    fn from(failure: SourceFailure) -> Self {
        Self::Source(failure)
    }
}

impl From<RevisionFailure> for Failed {
    fn from(failure: RevisionFailure) -> Self {
        Self::Revision(failure)
    }
}
