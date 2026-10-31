//! The `starmap` command's command-line grammar and help guide.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use crumbkit::commands::{COMMANDS_LIST, CommandsList};
use crumbkit::help::{Effects, Example, Guide, Leaf};
use crumbkit::{Call, DryRun, ViewFlags};
use crumbkit_usage::{SELF_USAGE, SelfUsage};
use starmap::Name;

use crate::check::CHECK;
use crate::explain::EXPLAIN;
use crate::init::INIT;

/// Write, check and explain the environment variables that an application
/// declares in starmap.toml.
#[derive(Debug, Parser)]
#[command(name = "starmap", version)]
pub struct Cli {
    /// The command to run.
    #[command(subcommand)]
    pub(crate) verb: Verb,

    /// The flags that choose how the result prints.
    #[command(flatten)]
    pub(crate) view: ViewFlags,
}

/// The `starmap` command's root verbs, leaves and their examples. `init`,
/// `check` and `explain` sit directly under the root, because a person
/// types `starmap check` as one step of their work.
pub const GUIDE: Guide = Guide::new()
    .root_verbs(
        &["init", "check", "explain"],
        "starmap acts on one file, starmap.toml, so its commands need no resource word.",
    )
    .leaves(&[
        Leaf::new(&["init"])
            .examples(&[
                Example::new(
                    &[],
                    "Write starmap.toml with a guess for each variable that src reads",
                ),
                Example::new(&["--dry-run"], "Print the guesses and write nothing"),
            ])
            .effects(Effects::additive())
            .document(INIT),
        Leaf::new(&["check"])
            .examples(&[
                Example::new(&[], "Check starmap.toml in the current directory"),
                Example::new(&["--json"], "Print the result of the check as JSON"),
            ])
            .effects(Effects::read_only())
            .document(CHECK),
        Leaf::new(&["explain"])
            .examples(&[
                Example::new(&[], "Explain every variable and its value in this shell"),
                Example::new(&["SHELF_MIRROR_TOKEN"], "Explain one variable"),
                Example::new(
                    &["SHELF_MIRROR_TOKEN", "--json"],
                    "Print one variable as JSON, where a withheld value has no text",
                ),
            ])
            .effects(Effects::read_only())
            .document(EXPLAIN),
        Leaf::new(&["commands", "list"])
            .examples(&[Example::new(&[], "List every command")])
            .effects(Effects::read_only())
            .document(COMMANDS_LIST),
        Leaf::new(&["self", "usage"])
            .examples(&[Example::new(&[], "Report the findings of the last 7 days")])
            .effects(Effects::read_only())
            .document(SELF_USAGE),
    ]);

/// The commands of `starmap`.
#[derive(Debug, Subcommand)]
pub(crate) enum Verb {
    /// Write starmap.toml with a guess for each variable that src reads.
    Init(InitArgs),
    /// Check that starmap.toml follows every rule of the file, and that it
    /// declares every variable that src reads.
    Check(CheckArgs),
    /// Describe each declared variable and the value it has in this shell.
    Explain(ExplainArgs),
    /// Explore starmap's commands.
    Commands {
        /// What to do with the commands.
        #[command(subcommand)]
        command: CommandsCommand,
    },
    /// Review starmap's own use.
    #[command(name = "self")]
    SelfCommand {
        /// What to review.
        #[command(subcommand)]
        command: SelfCommand,
    },
}

/// The commands for starmap's own commands.
#[derive(Debug, Subcommand)]
pub(crate) enum CommandsCommand {
    /// List a branch of starmap's commands with their arguments, examples and columns.
    List(CommandsList),
}

/// The commands that review starmap itself.
#[derive(Debug, Subcommand)]
pub(crate) enum SelfCommand {
    /// Report the patterns in starmap's usage log where callers waste effort.
    Usage(SelfUsage),
}

/// The file that a command reads.
#[derive(Debug, Args)]
pub(crate) struct SpecFile {
    /// Read the spec from this file
    #[arg(long, value_name = "FILE", default_value = SpecFile::DEFAULT)]
    pub(crate) spec: PathBuf,
}

impl SpecFile {
    /// The file that `--spec` names when it is not passed.
    pub(crate) const DEFAULT: &str = "starmap.toml";

    /// Returns the call of the command at `path` on the spec `file`. The
    /// call passes `--spec` only when `file` is not the default, so the
    /// usual call stays short, such as `starmap check`.
    pub(crate) fn call(path: &'static [&'static str], file: &str) -> Call {
        let call = Call::new(path);
        if file == Self::DEFAULT {
            call
        } else {
            call.value("spec", file)
        }
    }
}

/// The arguments of `starmap init`.
#[derive(Debug, Args)]
pub(crate) struct InitArgs {
    /// Write the spec to this file, and scan the src directory beside it
    #[arg(long, value_name = "FILE", default_value = SpecFile::DEFAULT)]
    pub(crate) spec: PathBuf,

    /// Print the guesses and write nothing. `crumbkit::parse` reads the
    /// flag, and `init` reads it from the output.
    #[command(flatten)]
    pub(crate) dry_run: DryRun,
}

/// The arguments of `starmap check`.
#[derive(Debug, Args)]
pub(crate) struct CheckArgs {
    /// The file that holds the spec.
    #[command(flatten)]
    pub(crate) file: SpecFile,
}

/// The arguments of `starmap explain`.
#[derive(Debug, Args)]
pub(crate) struct ExplainArgs {
    /// The variables to explain, separated by spaces; without names, every declared variable
    #[arg(value_name = "NAME")]
    pub(crate) names: Vec<Name>,

    /// The file that holds the spec.
    #[command(flatten)]
    pub(crate) file: SpecFile,
}
