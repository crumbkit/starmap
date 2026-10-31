//! The convention tests of the `starmap` command, which hold its command
//! tree, guide, help pages, examples and failures to crumbkit's conventions
//! with crumbkit-test (CLI-6).
//!
//! They live in an integration test so that the example check can run the
//! built binary, whose path cargo sets only for integration tests. Each
//! example runs in `crates/starmap/tests/example`, which holds the build
//! plan's illustrative `starmap.toml`, and each spec failure runs in
//! `crates/starmap/tests/spec`, which holds one file for each kind of spec
//! problem. `SNAPSHOTS=overwrite cargo test` writes, rewrites and deletes the
//! golden files.

#![cfg(test)]

use clap::CommandFactory as _;
use crumbkit::Code;
use crumbkit_test::{
    Allow, Budgets, Examples, Failing, Pages, PathBudgets, Recoveries, Rule, Twin, Twins,
    Unprovoked,
};
use starmap_cli::{Cli, GUIDE, codes};

/// The directory that holds the build plan's illustrative `starmap.toml`,
/// relative to this package.
const EXAMPLE: &str = "../starmap/tests/example";

/// The directory that holds a `starmap.toml` fixture for each kind of spec
/// problem, relative to this package.
const SPEC_FIXTURES: &str = "../starmap/tests/spec";

/// The `starmap` command's opt-outs from crumbkit's conventions, which
/// every check below reads.
const ALLOW: &[Allow] = &[Allow::new(
    Rule::Examples,
    "self usage",
    "The output names the day the report starts, which changes every day, and tests/usage.rs checks the usage log instead.",
)];

/// The budgets of the `-h` pages, which are crumbkit's defaults.
const BUDGETS: Budgets = Budgets::new();

/// The budgets of the paths through the help, which are crumbkit's
/// defaults.
const PATH_BUDGETS: PathBudgets<'_> = PathBudgets::new("tests/golden/examples");

/// The variables that each example run sets: a port, and a token whose value
/// explain must withhold. The mode variable stays unset, so its default
/// decides the mode.
const ENV: &[(&str, &str)] = &[
    ("SHELF_PORT", "8080"),
    ("SHELF_MIRROR_TOKEN", "a-token-that-explain-never-prints"),
];

/// The built binary and the directory that holds the example golden files.
const EXAMPLES: Examples<'_> =
    Examples::new(env!("CARGO_BIN_EXE_starmap"), "tests/golden/examples")
        .env(ENV)
        .current_dir(EXAMPLE)
        .allow(ALLOW);

#[test]
fn every_command_and_argument_has_one_name() {
    crumbkit_test::check_names(&Cli::command(), Pages::rendered(&GUIDE), ALLOW);
}

#[test]
fn every_command_is_a_root_verb_or_a_resource_followed_by_list_or_usage() {
    crumbkit_test::check_grammar(&Cli::command(), Pages::rendered(&GUIDE), &["usage"], ALLOW);
}

#[test]
fn every_command_declares_its_effects() {
    crumbkit_test::check_effects(&Cli::command(), &GUIDE, ALLOW);
}

#[test]
fn every_list_command_pages() {
    crumbkit_test::check_listings(&Cli::command(), ALLOW);
}

#[test]
fn every_option_with_many_values_accepts_repeats_and_commas() {
    crumbkit_test::check_filters(&Cli::command(), ALLOW);
}

#[test]
fn every_command_leaf_and_topic_meets_the_guide_rules() {
    crumbkit_test::check_guide(&Cli::command(), &GUIDE, ALLOW);
}

#[test]
fn every_exit_code_meets_the_exit_code_rules() {
    crumbkit_test::check_exit_codes(&Cli::command(), &GUIDE, ALLOW);
}

#[test]
fn every_failure_code_is_declared_once_and_spelled_by_the_rules() {
    crumbkit_test::check_codes(&GUIDE, &[codes::ALL]);
}

#[test]
fn every_help_page_fits_its_size_budget() {
    crumbkit_test::check_help_sizes(&Cli::command(), Pages::rendered(&GUIDE), &BUDGETS);
}

#[test]
fn every_path_through_the_help_fits_its_weight_budget() {
    crumbkit_test::check_path_weights(&Cli::command(), Pages::rendered(&GUIDE), &PATH_BUDGETS);
}

#[test]
fn every_help_page_matches_its_golden_file() {
    crumbkit_test::check_help_goldens(
        &Cli::command(),
        Pages::rendered(&GUIDE),
        "tests/golden/help",
    );
}

#[test]
fn every_document_schema_matches_its_golden_file() {
    crumbkit_test::check_schema_goldens(&Cli::command(), &GUIDE, "tests/golden/schemas");
}

#[test]
fn every_document_is_named_after_the_path_of_the_command_that_prints_it() {
    crumbkit_test::check_document_ids(&Cli::command(), &GUIDE, ALLOW);
}

#[test]
fn every_crumbkit_command_sits_at_its_fixed_path() {
    crumbkit_test::check_mounts(&Cli::command(), &GUIDE);
}

#[test]
fn every_example_in_the_guide_runs_and_prints_its_golden_output() {
    crumbkit_test::check_examples(&Cli::command(), &GUIDE, &EXAMPLES);
}

#[test]
fn every_json_example_cuts_or_refuses_a_result_past_its_output_budget() {
    crumbkit_test::check_output_budget(&Cli::command(), &GUIDE, &EXAMPLES);
}

#[test]
fn every_command_matches_the_checked_in_command_reference() {
    crumbkit_test::check_command_reference(env!("CARGO_BIN_EXE_starmap"), "docs/commands.md");
}

#[test]
fn every_opt_out_matches_the_opt_out_report() {
    crumbkit_test::check_opt_outs(
        &Cli::command(),
        &GUIDE,
        ALLOW,
        &BUDGETS,
        Some(&PATH_BUDGETS),
        "tests/golden",
    );
}

/// Returns the command line `words`, which runs in [`SPEC_FIXTURES`] and
/// fails with `code`.
const fn spec(code: &'static Code, words: &'static [&'static str]) -> Failing<'static> {
    Failing::new(code, words).dir(SPEC_FIXTURES)
}

/// The command lines that fail, one for each declared code that a command
/// line provokes. Each spec failure checks one fixture of
/// [`SPEC_FIXTURES`], whose first problem has the failure's kind.
const STARTS: &[Failing<'static>] = &[
    spec(&codes::SPEC_NOT_FOUND, &["check", "--spec", "missing.toml"]),
    spec(&codes::SPEC_UNREADABLE, &["check", "--spec", "."]),
    spec(&codes::SPEC_SYNTAX, &["check", "--spec", "syntax.toml"]),
    spec(
        &codes::SPEC_UNKNOWN_FIELD,
        &["check", "--spec", "unknown_field.toml"],
    ),
    spec(
        &codes::SPEC_MISSING_FIELD,
        &["check", "--spec", "missing_field.toml"],
    ),
    spec(
        &codes::SPEC_BAD_VALUE,
        &["check", "--spec", "bad_value.toml"],
    ),
    spec(&codes::SPEC_BAD_NAME, &["check", "--spec", "bad_name.toml"]),
    spec(&codes::SPEC_GUESS, &["check", "--spec", "guess.toml"]),
    spec(
        &codes::SPEC_BAD_DEFAULT,
        &["check", "--spec", "bad_default.toml"],
    ),
    spec(
        &codes::SPEC_BAD_RANGE,
        &["check", "--spec", "bad_range.toml"],
    ),
    spec(
        &codes::SPEC_EMPTY_ENUMERATION,
        &["check", "--spec", "empty_enumeration.toml"],
    ),
    spec(
        &codes::SPEC_SHOWN_SECRET,
        &["check", "--spec", "shown_secret.toml"],
    ),
    spec(
        &codes::SPEC_SECRET_FORM,
        &["check", "--spec", "secret_form.toml"],
    ),
    spec(
        &codes::SPEC_BIND_MISMATCH,
        &["check", "--spec", "bind_mismatch.toml"],
    ),
    spec(
        &codes::SPEC_BAD_BYTES,
        &["check", "--spec", "bad_bytes.toml"],
    ),
    spec(
        &codes::SPEC_UNKNOWN_PLACEHOLDER,
        &["check", "--spec", "unknown_placeholder.toml"],
    ),
    spec(
        &codes::SPEC_MODE_UNDECLARED,
        &["check", "--spec", "mode_undeclared.toml"],
    ),
    spec(
        &codes::SPEC_MODE_SOURCE,
        &["check", "--spec", "mode_source.toml"],
    ),
    Failing::new(&codes::VARIABLE_UNKNOWN, &["explain", "SHELF_THEME"]),
];

/// The declared codes that no command line in this file provokes.
const UNPROVOKED: &[Unprovoked<'_>] = &[Unprovoked::new(
    &codes::SPEC_TOO_LARGE,
    "Only a file over 256 KiB provokes it, which is too large to keep as a fixture, so tests/spec.rs writes one and checks the failure.",
)];

/// The settings of the recovery check.
const RECOVERIES: Recoveries<'_> = Recoveries::new(EXAMPLES, &[codes::ALL])
    .starts(STARTS)
    .unprovoked(UNPROVOKED);

#[test]
fn every_recovery_leads_to_a_command_that_runs() {
    crumbkit_test::check_recoveries(&Cli::command(), &GUIDE, &RECOVERIES, "tests/golden");
}

/// The line that each spec failure's twin runs in [`SPEC_FIXTURES`]: the
/// same command with the valid example file in place of the fixture.
const VALID: &[&str] = &["check", "--spec", "../example/starmap.toml"];

/// One twin for each start. Each differs from its refused line in one
/// word, so that word is the reason for the refusal (TEST-1).
const PAIRS: &[Twin<'static>] = &[
    Twin::new(STARTS[0], VALID),
    Twin::new(STARTS[1], VALID),
    Twin::new(STARTS[2], VALID),
    Twin::new(STARTS[3], VALID),
    Twin::new(STARTS[4], VALID),
    Twin::new(STARTS[5], VALID),
    Twin::new(STARTS[6], VALID),
    Twin::new(STARTS[7], VALID),
    Twin::new(STARTS[8], VALID),
    Twin::new(STARTS[9], VALID),
    Twin::new(STARTS[10], VALID),
    Twin::new(STARTS[11], VALID),
    Twin::new(STARTS[12], VALID),
    Twin::new(STARTS[13], VALID),
    Twin::new(STARTS[14], VALID),
    Twin::new(STARTS[15], VALID),
    Twin::new(STARTS[16], VALID),
    Twin::new(STARTS[17], VALID),
    Twin::new(STARTS[18], &["explain", "SHELF_PORT"]),
];

/// The settings of the twin check.
const TWINS: Twins<'_> = Twins::new(EXAMPLES, &[codes::ALL])
    .pairs(PAIRS)
    .unprovoked(UNPROVOKED);

#[test]
fn every_declared_code_has_a_twin_that_the_command_accepts() {
    crumbkit_test::check_twins(&Cli::command(), &GUIDE, &TWINS);
}
