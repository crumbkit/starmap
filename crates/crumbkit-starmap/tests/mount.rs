//! A tool that mounts `self environment list` as the README describes: the
//! command parses, runs against the tool's embedded declaration and prints
//! its document.

#![cfg(test)]

use std::ffi::OsString;

use clap::{Parser, Subcommand};
use crumbkit::help::{Group, Help};
use crumbkit::{Sink, Tool, ViewFlags, finish_to, try_parse_to};
use crumbkit_starmap::{SELF_ENVIRONMENT_LIST, SelfEnvironment, environment, list_leaf};

/// The declaration that the tool embeds.
const SPEC: &str = include_str!("../../starmap/tests/example/starmap.toml");

/// A tool with the `self environment list` command and nothing else.
#[derive(Debug, Parser)]
#[command(name = "shelf")]
struct Cli {
    /// The command to run.
    #[command(subcommand)]
    command: Root,

    /// The flags that choose how the result prints.
    #[command(flatten)]
    view: ViewFlags,
}

/// The root commands of the tool.
#[derive(Debug, Subcommand)]
enum Root {
    /// Review the tool itself.
    #[command(name = "self")]
    SelfCommand {
        /// What to review.
        #[command(subcommand)]
        command: SelfCommand,
    },
}

/// The commands that review the tool itself.
#[derive(Debug, Subcommand)]
enum SelfCommand {
    /// Work with the tool's environment variables.
    Environment {
        /// What to do with them.
        #[command(subcommand)]
        verb: EnvironmentVerb,
    },
}

/// What to do with the tool's environment variables.
#[derive(Debug, Subcommand)]
enum EnvironmentVerb {
    /// List the variables that the tool declares and their values.
    List(SelfEnvironment),
}

/// The tool's help record.
const TOOL: Tool = Tool::new().help(
    Help::new()
        .groups(&[Group::new(&[], &[])])
        .leaves(&[list_leaf(&["self", "environment", "list"])]),
);

/// Runs `shelf self environment list` with `flags` and `vars` as the process
/// environment, and returns what it printed to standard output.
fn list(flags: &[&str], vars: &[(&str, &str)]) -> String {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut sink = Sink::new(&mut out, &mut err);
    let mut args = vec!["shelf", "self", "environment", "list"];
    args.extend(flags);
    let (cli, mut run) = try_parse_to::<Cli>(&mut sink, Cli::command_for_test(), &TOOL, args)
        .unwrap_or_else(|_| panic!("the command line parses"));
    let vars: Vec<(OsString, OsString)> = vars
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect();
    let Root::SelfCommand {
        command: SelfCommand::Environment {
            verb: EnvironmentVerb::List(args),
        },
    } = cli.command;
    let _ = cli.view;
    let result = environment(&mut run, SPEC, vars, &args);
    let code = finish_to(&mut sink, run, SELF_ENVIRONMENT_LIST, result);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    String::from_utf8(out).unwrap()
}

impl Cli {
    /// Returns the clap command of the tool.
    fn command_for_test() -> clap::Command {
        <Self as clap::CommandFactory>::command()
    }
}

#[test]
fn the_command_prints_each_declared_variable_and_crumbkits_as_json() {
    let printed = list(
        &["--json", "--long"],
        &[("SHELF_PORT", "8080"), ("SHELF_PROGRAM", "flightdeck")],
    );
    let document: serde_json::Value = serde_json::from_str(&printed).unwrap();
    let rows = document["data"].as_array().unwrap();
    let state = |name: &str| {
        rows.iter()
            .find(|row| row["name"] == name)
            .map(|row| row["state"].clone())
    };

    assert_eq!(state("SHELF_PORT"), Some("set".into()));
    assert_eq!(state("SHELF_PROGRAM"), Some("set".into()));
    assert_eq!(state("SHELF_MIRROR_TOKEN"), Some("unset".into()));
    assert!(!printed.contains("hunter2"));
}

#[test]
fn the_command_prints_a_table_with_the_columns_of_explain() {
    let printed = list(&[], &[("SHELF_PORT", "8080")]);
    let header = printed.lines().next().unwrap();

    assert!(header.starts_with("NAME"), "{header}");
    assert!(printed.contains("SHELF_PORT"));
    assert!(printed.contains("8080"));
}
