//! The `starmap` command's command-line grammar and tool record.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::builder::NonEmptyStringValueParser;
use clap::{Args, Parser, Subcommand};
use crumbkit::commands::{COMMANDS_LIST, CommandsList};
use crumbkit::completion::{
    COMPLETION_CHECK, COMPLETION_INSTALL, COMPLETION_PRINT, CompletionCheck, CompletionInstall,
    CompletionPrint,
};
use crumbkit::help::{Effects, Example, Goal, Group, Help, Leaf};
use crumbkit::schemas::{SCHEMAS_LIST, SCHEMAS_SHOW, SchemasList, SchemasShow};
use crumbkit::tool::{Exit, Failures};
use crumbkit::{Call, DryRun, NoInput, Tool, ViewFlags};
use crumbkit_skills::{
    SKILLS_CHECK, SKILLS_INSTALL, SKILLS_PRINT, Skill, SkillsCheck, SkillsInstall,
};
use crumbkit_usage::{SELF_USAGE, SelfUsage};
use starmap::Name;
use starmap_resolve::WorkspaceName;

use crate::check::CHECK;
use crate::diff::DIFF;
use crate::explain::EXPLAIN;
use crate::init::INIT;

/// Write, check, explain and run the environment variables that an
/// application declares in starmap.toml.
#[derive(Debug, Parser)]
#[command(name = "starmap", version)]
pub struct Cli {
    /// The command to run.
    #[command(subcommand)]
    pub(crate) verb: Verb,

    /// The flags that choose how the result prints.
    #[command(flatten)]
    pub(crate) view: ViewFlags,

    /// Ask no question. `run` then refuses a secret that it would ask for,
    /// and `crumbkit::parse` reads the flag.
    #[command(flatten)]
    pub(crate) no_input: NoInput,
}

/// The `starmap` command's goal headings, root verbs, leaves and their
/// examples. `init`, `check`, `explain`, `diff` and `run` sit directly under
/// the root, because a person types `starmap check` as one step of their
/// work.
pub const TOOL: Tool = Tool::new().help(HELP).failures(FAILURES);

/// The goal headings, root verbs and leaves of the `starmap` command.
const HELP: Help = Help::new()
    .groups(&[Group::new(
        &[],
        &[
            Goal::new("Write and check starmap.toml", &["init", "check"]),
            Goal::new("Run a program with its environment", &["run"]),
            Goal::new("Read the variables", &["explain", "diff"]),
            Goal::new("Explore commands", &["commands", "schemas", "self"]),
            Goal::new("Install starmap's skill", &["skills"]),
            Goal::new("Set up shell completion", &["completion"]),
        ],
    )])
    .root_verbs(
        &["init", "check", "explain", "diff", "run"],
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
        Leaf::new(&["diff"])
            .examples(&[Example::new(
                &["main"],
                "List each variable that changed since the spec on main",
            )])
            .effects(Effects::read_only())
            .document(DIFF),
        // run may add an item to the Keychain when a person agrees to save
        // a typed secret, and it changes nothing else.
        Leaf::new(&["run"])
            .examples(&[
                Example::new(
                    &["--", "cargo", "run"],
                    "Run the app with the environment that starmap.toml declares",
                ),
                Example::new(
                    &["--set", "SHELF_DEBUG=true", "--", "cargo", "run"],
                    "Run the app with one setting changed for this run",
                ),
            ])
            .effects(Effects::additive())
            .passes_on_exit("the program after --"),
        Leaf::new(&["commands", "list"])
            .examples(&[Example::new(&[], "List every command")])
            .effects(Effects::read_only())
            .document(COMMANDS_LIST),
        Leaf::new(&["schemas", "list"])
            .examples(&[Example::new(&[], "List every document that starmap prints")])
            .effects(Effects::read_only())
            .document(SCHEMAS_LIST),
        Leaf::new(&["schemas", "show"])
            .examples(&[Example::new(
                &["starmap/init/v1"],
                "Print the JSON Schema of the document that init prints",
            )])
            .effects(Effects::read_only())
            .document(SCHEMAS_SHOW),
        Leaf::new(&["self", "usage"])
            .examples(&[Example::new(&[], "Report the findings of the last 7 days")])
            .effects(Effects::read_only())
            .document(SELF_USAGE),
        Leaf::new(&["skills", "install"])
            .examples(&[
                Example::new(
                    &[],
                    "Install starmap's skill into each agent skills directory that exists",
                ),
                Example::new(
                    &["--force"],
                    "Install it, replacing a file or a link that install did not write",
                ),
            ])
            .effects(Effects::additive().idempotent())
            .document(SKILLS_INSTALL),
        Leaf::new(&["skills", "check"])
            .examples(&[Example::new(
                &[],
                "Report each agent skills directory whose copy is missing or differs",
            )])
            .effects(Effects::read_only())
            .document(SKILLS_CHECK),
        Leaf::new(&["skills", "print"])
            .examples(&[Example::new(&[], "Print starmap's skill")])
            .effects(Effects::read_only())
            .document(SKILLS_PRINT),
        Leaf::new(&["completion", "install"])
            .examples(&[
                Example::new(
                    &["--shell", "fish"],
                    "Install starmap's completion for fish",
                ),
                Example::new(
                    &["--shell", "zsh", "--force"],
                    "Install it for zsh, replacing a file or a link that install did not write",
                ),
            ])
            .effects(Effects::additive().idempotent())
            .document(COMPLETION_INSTALL),
        Leaf::new(&["completion", "check"])
            .examples(&[Example::new(
                &["--shell", "zsh"],
                "Report zsh's completion file when it is missing or differs",
            )])
            .effects(Effects::read_only())
            .document(COMPLETION_CHECK),
        Leaf::new(&["completion", "print"])
            .examples(&[Example::new(
                &["--shell", "fish"],
                "Print the registration script for fish",
            )])
            .effects(Effects::read_only())
            .document(COMPLETION_PRINT),
    ]);

/// starmap's agent skill, which `starmap skills install` writes into the
/// agent skills directories.
pub(crate) const SKILL: Skill = Skill::new(
    "starmap",
    env!("CARGO_PKG_VERSION"),
    include_str!("../skill/SKILL.md"),
);

/// The exit codes that `starmap run` passes on from its program.
const FAILURES: Failures = Failures::new().exits(&[Exit::range(
    3,
    255,
    "starmap run passes on the exit code of the program it runs. The program's exit codes 0, 1 and 2 pass on as well, and are then the same numbers as starmap's own success, failure and refused command line.",
)
.commands(&[&["run"]])]);

/// The commands of `starmap`.
#[derive(Debug, Subcommand)]
pub(crate) enum Verb {
    /// Write starmap.toml with a guess for each variable that src reads.
    ///
    /// The scan skips test code: files named tests.rs or ending in
    /// _tests.rs, a file that begins with #![cfg(test)], and the item after
    /// #[cfg(test)]. check scans src the same way.
    Init(InitArgs),
    /// Check that starmap.toml follows every rule of the file, and that it
    /// declares every variable that src reads.
    ///
    /// check counts a literal name passed to var or var_os, a
    /// #[starmap("NAME")] field, and clap's env argument, as in
    /// #[arg(env = "NAME")]; a bare clap env reads the upper-case form of
    /// its field's name. It does not count a name held in a constant.
    ///
    /// A read whose name format! builds from a name, an underscore and one
    /// trailing placeholder, such as format!("SHELF_DB_URL_{region}"),
    /// reads a member of that family, and check fails when the file
    /// declares no such family. check passes on any other format! name and
    /// names the place of each one it cannot resolve.
    Check(CheckArgs),
    /// Describe each declared variable and the value it has in this shell.
    Explain(ExplainArgs),
    /// List each variable that was added, changed or removed since a git
    /// revision's spec.
    ///
    /// diff compares the spec with the file at the same path in the
    /// revision. A revision without the file counts as an empty spec.
    Diff(DiffArgs),
    /// Run a program with the environment that starmap.toml declares.
    ///
    /// run resolves every variable, checks the values and starts the
    /// program with each declared variable set or removed. A value comes
    /// from --set, then the shell, then .env beside the spec or the file
    /// that a pointer names, then a setting's path bind, then the default.
    /// A secret comes from its bind, or from the shell or the Keychain when
    /// it has none; at a terminal, run asks for a missing one and offers to save it to the
    /// Keychain. .env and --set never give a secret.
    Run(RunArgs),
    /// Explore starmap's commands.
    Commands {
        /// What to do with the commands.
        #[command(subcommand)]
        command: CommandsCommand,
    },
    /// Explore the JSON documents that starmap prints.
    Schemas {
        /// What to do with the documents.
        #[command(subcommand)]
        command: SchemasCommand,
    },
    /// Review starmap's own use.
    #[command(name = "self")]
    SelfCommand {
        /// What to review.
        #[command(subcommand)]
        command: SelfCommand,
    },
    /// Give coding agents starmap's skill.
    Skills {
        /// What to do with the skill.
        #[command(subcommand)]
        command: SkillsCommand,
    },
    /// Set up starmap's completion in a shell.
    Completion {
        /// What to do with the completion.
        #[command(subcommand)]
        command: CompletionCommand,
    },
}

/// The commands for starmap's own commands.
#[derive(Debug, Subcommand)]
pub(crate) enum CommandsCommand {
    /// List a branch of starmap's commands with their arguments, examples and columns.
    List(CommandsList),
}

/// The commands for the JSON documents that starmap prints.
#[derive(Debug, Subcommand)]
pub(crate) enum SchemasCommand {
    /// List the JSON documents that starmap prints, with a description of each.
    List(SchemasList),
    /// Print the JSON Schema of each document.
    Show(SchemasShow),
}

/// The commands that review starmap itself.
#[derive(Debug, Subcommand)]
pub(crate) enum SelfCommand {
    /// Report the patterns in starmap's usage log where callers waste effort.
    Usage(SelfUsage),
}

/// The commands for starmap's agent skill.
#[derive(Debug, Subcommand)]
pub(crate) enum SkillsCommand {
    /// Install starmap's skill into each agent skills directory that exists.
    Install(SkillsInstall),
    /// Check that each installed copy of starmap's skill matches this starmap.
    Check(SkillsCheck),
    /// Print starmap's skill.
    Print,
}

/// The commands for starmap's shell completion.
#[derive(Debug, Subcommand)]
pub(crate) enum CompletionCommand {
    /// Install starmap's completion where the shell loads it.
    Install(CompletionInstall),
    /// Check that starmap's installed completion file matches this starmap.
    Check(CompletionCheck),
    /// Print starmap's completion registration script for a shell.
    Print(CompletionPrint),
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
    /// Write the spec to this file, and scan the src directory beside it; for a pointer, write the file it names and scan the directories it lists
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

/// The arguments of `starmap diff`.
#[derive(Debug, Args)]
pub(crate) struct DiffArgs {
    /// The git revision to compare with, such as main or HEAD~3
    #[arg(value_name = "REVISION", value_parser = NonEmptyStringValueParser::new())]
    pub(crate) revision: String,

    /// The file that holds the spec.
    #[command(flatten)]
    pub(crate) file: SpecFile,
}

/// The arguments of `starmap run`.
#[derive(Debug, Args)]
pub(crate) struct RunArgs {
    /// The file that holds the spec.
    #[command(flatten)]
    pub(crate) file: SpecFile,

    /// Name the Keychain items of this checkout after this workspace; default: the checkout's directory name
    #[arg(long, value_name = "NAME")]
    pub(crate) workspace: Option<WorkspaceName>,

    /// Give a setting or inherited variable this value for this run, as NAME=VALUE; repeat it for more
    #[arg(long, value_name = "NAME=VALUE", value_parser = parse_assignment)]
    pub(crate) set: Vec<Assignment>,

    /// The program to run and its arguments, after --
    #[arg(value_name = "PROGRAM", required = true, last = true)]
    pub(crate) program: Vec<OsString>,
}

/// One `--set NAME=VALUE`.
#[derive(Debug, Clone)]
pub(crate) struct Assignment {
    /// The variable.
    pub(crate) name: Name,
    /// The value.
    pub(crate) value: String,
}

/// Parses `NAME=VALUE` into an [`Assignment`].
fn parse_assignment(text: &str) -> Result<Assignment, String> {
    let (name, value) = text
        .split_once('=')
        .ok_or_else(|| "write the value as NAME=VALUE".to_owned())?;
    let name = name.parse::<Name>().map_err(|error| error.to_string())?;
    Ok(Assignment {
        name,
        value: value.to_owned(),
    })
}
