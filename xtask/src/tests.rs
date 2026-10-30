//! Tests for xtask's command line.

use clap::Parser;

use super::Cli;

#[test]
fn check_parses_as_the_check_command() {
    assert_eq!(
        Cli::try_parse_from(["cargo xtask", "check"]).unwrap(),
        Cli::Check
    );
}

#[test]
fn an_unknown_command_is_refused() {
    assert!(Cli::try_parse_from(["cargo xtask", "lint"]).is_err());
}

#[test]
fn a_missing_command_is_refused() {
    assert!(Cli::try_parse_from(["cargo xtask"]).is_err());
}
