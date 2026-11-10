//! The convention tests of the `starmap` command, which hold its command
//! tree, tool record, help pages, examples and failures to crumbkit's conventions
//! with crumbkit-test (CLI-6).
//!
//! They live in an integration test so that the example check can run the
//! built binary, whose path cargo sets only for integration tests. Each
//! example runs in a fresh copy of `crates/starmap/tests/example`, which
//! holds the example `starmap.toml` and the source of an
//! application that reads its variables. Each spec failure runs in
//! `crates/starmap/tests/spec`, which holds one file for each kind of spec
//! problem, and each source failure runs in `tests/fixtures`. `SNAPSHOTS=overwrite cargo test` writes, rewrites and deletes the
//! golden files.

#![cfg(test)]

use clap::CommandFactory as _;
use crumbkit::Code;
use crumbkit_test::{
    Allow, Examples, Failing, PathBudgets, Recoveries, Rule, Suite, Twin, Twins, Unprovoked,
};
use starmap_cli::{Cli, TOOL, codes};

/// The directory that holds the example `starmap.toml`,
/// relative to this package.
const EXAMPLE: &str = "../starmap/tests/example";

/// The directory that holds a `starmap.toml` fixture for each kind of spec
/// problem, relative to this package.
const SPEC_FIXTURES: &str = "../starmap/tests/spec";

/// The directory that holds an application for each failure of its
/// source, relative to this package.
const SOURCE_FIXTURES: &str = "tests/fixtures";

/// The directory that holds the specs and `.env` files of `run`, relative
/// to this package.
const RUN_FIXTURES: &str = "tests/fixtures/run";

/// The reason that the examples of `run` do not run in the example
/// directory.
const RUN_RESOLVES: &str = "The example directory's spec reads a secret through op and keeps a generated value in the Keychain, which a test must not touch, and the examples run cargo, so tests/run.rs runs the command against fixtures without either and checks what the program receives.";

/// The reason that the failures of `run` have no twin here.
const RUN_TWINS: &str = "The twin check runs each admitted line with --json, which run refuses after it reads the spec, .env and --set, because the program takes the terminal, so src/run/tests.rs pins each refusal of run beside an admitted line that differs in one value.";

/// The reason that the failures of `run` after resolution are not
/// provoked here.
const RUN_RESOLVES_FIRST: &str = "The recovery and twin checks run each line with --json, which run refuses before it asks a secret store, so only a text run reaches this failure, and src/run/tests.rs provokes it beside an admitted line.";

/// The reason that the examples of `init` do not run in the example
/// directory.
const INIT_RUNS_ELSEWHERE: &str = "The example directory holds starmap.toml, which init refuses to replace, so tests/init.rs runs init in a checkout without one and pins its output.";

/// The reason that the example of `diff` does not run in the example
/// directory, and that no command line in this file provokes a revision
/// failure that needs git.
const DIFF_NEEDS_GIT: &str = "Each example runs in a fresh copy of the example directory, which is not a git repository, so tests/diff.rs builds a repository with a committed spec and pins diff's output and its revision failures there.";

/// The `starmap` command's opt-outs from crumbkit's conventions, which
/// every check below reads.
const ALLOW: &[Allow] = &[
    Allow::new(
        Rule::Examples,
        "self usage",
        "The output names the day the report starts, which changes every day, and tests/usage.rs checks the usage log instead.",
    ),
    Allow::example("init", &[], INIT_RUNS_ELSEWHERE),
    Allow::example("init", &["--dry-run"], INIT_RUNS_ELSEWHERE),
    Allow::example("diff", &["main"], DIFF_NEEDS_GIT),
    Allow::example("run", &["--", "cargo", "run"], RUN_RESOLVES),
    Allow::untwinned(STARTS[25], RUN_TWINS),
    Allow::untwinned(STARTS[26], RUN_TWINS),
    Allow::untwinned(STARTS[27], RUN_TWINS),
    Allow::untwinned(STARTS[28], RUN_TWINS),
    Allow::new(
        Rule::Items,
        "run",
        "The words after -- are the command line of the one program that run starts, and run changes at most one Keychain item, after the person agrees.",
    ),
    Allow::option(
        Rule::RepeatsAndCommas,
        "run",
        "--set",
        "A value of --set is any text, which may hold commas, so each use gives one NAME=VALUE whole.",
    ),
    Allow::example(
        "run",
        &["--set", "SHELF_DEBUG=true", "--", "cargo", "run"],
        RUN_RESOLVES,
    ),
    Allow::untwinned(
        DASH_REVISION,
        "Every revision that diff accepts needs a git repository, and the example directory is not one, so tests/diff.rs pins an accepted revision there.",
    ),
];

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
        .fresh_dir(EXAMPLE)
        .allow(ALLOW);

#[test]
fn the_root_help_lists_the_commands_under_four_goal_headings_in_order() {
    let goals: Vec<(&str, &[&str])> = TOOL
        .get_help()
        .get_groups()
        .iter()
        .filter(|group| group.path().is_empty())
        .flat_map(|group| group.goals())
        .map(|goal| (goal.heading(), goal.commands()))
        .collect();
    assert_eq!(
        goals,
        [
            ("Write and check starmap.toml", &["init", "check"][..]),
            ("Run a program with its environment", &["run"][..]),
            ("Read the variables", &["explain", "diff"][..]),
            ("Explore commands", &["commands", "schemas", "self"][..]),
        ]
    );
}

/// Returns the command line `words`, which runs in [`SPEC_FIXTURES`] and
/// fails with `code`.
const fn spec(code: &'static Code, words: &'static [&'static str]) -> Failing<'static> {
    Failing::new(code, words).dir(SPEC_FIXTURES)
}

/// Returns the command line `words`, which runs in [`SOURCE_FIXTURES`] and
/// fails with `code`.
const fn source(code: &'static Code, words: &'static [&'static str]) -> Failing<'static> {
    Failing::new(code, words).dir(SOURCE_FIXTURES)
}

/// Returns the command line `words`, which runs in [`RUN_FIXTURES`] and
/// fails with `code`.
const fn run(code: &'static Code, words: &'static [&'static str]) -> Failing<'static> {
    Failing::new(code, words).dir(RUN_FIXTURES)
}

/// The command lines that fail, one for each declared code that a command
/// line provokes. Each spec failure checks one fixture of
/// [`SPEC_FIXTURES`], whose first problem has the failure's kind. The
/// failures that lead to `init` run in a fresh copy of [`EXAMPLE`], so the
/// file that init writes is gone after the chain.
const STARTS: &[Failing<'static>] = &[
    Failing::new(&codes::SPEC_NOT_FOUND, &["check", "--spec", "missing.toml"]),
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
        &codes::SPEC_WITHHELD_TYPE,
        &["check", "--spec", "withheld_type.toml"],
    ),
    spec(
        &codes::SPEC_WITHHELD_DEFAULT,
        &["check", "--spec", "withheld_default.toml"],
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
    Failing::new(&codes::SPEC_EXISTS, &["init", "--spec", "starmap.toml"]),
    Failing::new(
        &codes::SPEC_UNWRITABLE,
        &["init", "--spec", "missing/starmap.toml"],
    ),
    source(
        &codes::VARIABLE_UNDECLARED,
        &["check", "--spec", "undeclared/starmap.toml"],
    ),
    source(
        &codes::SOURCE_UNREADABLE,
        &["check", "--spec", "unreadable/starmap.toml"],
    ),
    DASH_REVISION,
    run(
        &codes::DOTENV_SECRET,
        &[
            "run",
            "--no-input",
            "--spec",
            "dotenv-secret/starmap.toml",
            "--",
            "/usr/bin/true",
        ],
    ),
    run(
        &codes::DOTENV_SYNTAX,
        &[
            "run",
            "--no-input",
            "--spec",
            "dotenv-syntax/starmap.toml",
            "--",
            "/usr/bin/true",
        ],
    ),
    run(
        &codes::DOTENV_UNREADABLE,
        &[
            "run",
            "--no-input",
            "--spec",
            "dotenv-unreadable/starmap.toml",
            "--",
            "/usr/bin/true",
        ],
    ),
    run(
        &codes::VARIABLE_SECRET,
        &[
            "run",
            "--no-input",
            "--set",
            "SHELF_UPSTREAM_TOKEN=x",
            "--",
            "/usr/bin/true",
        ],
    ),
    spec(
        &codes::SPEC_POINTER_DECLARES,
        &["check", "--spec", "pointer_declares.toml"],
    ),
    spec(
        &codes::SPEC_POINTER_CHAIN,
        &["check", "--spec", "pointer_chain.toml"],
    ),
];

/// A revision that starts with `-`, which diff refuses before git runs, so
/// the line fails the same way outside a git repository.
const DASH_REVISION: Failing<'static> =
    Failing::new(&codes::REVISION_NOT_FOUND, &["diff", "--", "-x"]);

/// The declared codes that no command line in this file provokes.
const UNPROVOKED: &[Unprovoked<'_>] = &[
    Unprovoked::new(
        &codes::SPEC_TOO_LARGE,
        "Only a file over 256 KiB provokes it, which is too large to keep as a fixture, so tests/spec.rs writes one and checks the failure.",
    ),
    Unprovoked::new(&codes::REVISION_NO_REPOSITORY, DIFF_NEEDS_GIT),
    Unprovoked::new(
        &codes::REVISION_UNREADABLE,
        "Only a git that cannot start or cannot read the repository provokes it, which no fixture can set up portably.",
    ),
    Unprovoked::new(&codes::VARIABLE_MISSING, RUN_RESOLVES_FIRST),
    Unprovoked::new(&codes::VARIABLE_INVALID, RUN_RESOLVES_FIRST),
    Unprovoked::new(&codes::BIND_NO_PROVIDER, RUN_RESOLVES_FIRST),
    Unprovoked::new(&codes::BIND_NO_PLACE, RUN_RESOLVES_FIRST),
    Unprovoked::new(&codes::SECRET_UNBOUND, RUN_RESOLVES_FIRST),
    Unprovoked::new(
        &codes::SECRET_UNREADABLE,
        "Only 1Password or the Keychain provokes it, which a test must not touch, so starmap-resolve's tests provoke each SecretError through a fake op program and a fake resolver, and src/run/tests.rs checks how run reports one.",
    ),
];

/// The settings of the recovery check.
const RECOVERIES: Recoveries<'_> = Recoveries::new(EXAMPLES, &[codes::ALL])
    .starts(STARTS)
    .unprovoked(UNPROVOKED);

/// The line that each spec failure's twin runs in [`SPEC_FIXTURES`]: the
/// same command with the valid example file in place of the fixture.
const VALID: &[&str] = &["check", "--spec", "../example/starmap.toml"];

/// The line that each source failure's twin runs in [`SOURCE_FIXTURES`]:
/// the same command with the example application in place of the fixture.
const VALID_SOURCE: &[&str] = &[
    "check",
    "--spec",
    "../../../starmap/tests/example/starmap.toml",
];

/// The line that each refused `init` twins with in a fresh copy of
/// [`EXAMPLE`]: init with a spec path that is free and writable.
const NEW_SPEC: &[&str] = &["init", "--spec", "new.toml"];

/// One twin for each start. Each differs from its refused line in one
/// word, so that word is the reason for the refusal (TEST-1).
const PAIRS: &[Twin<'static>] = &[
    Twin::new(STARTS[0], &["check", "--spec", "starmap.toml"]),
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
    Twin::new(STARTS[18], VALID),
    Twin::new(STARTS[19], &["explain", "SHELF_PORT"]),
    Twin::new(STARTS[20], NEW_SPEC),
    Twin::new(STARTS[21], NEW_SPEC),
    Twin::new(STARTS[22], VALID_SOURCE),
    Twin::new(STARTS[23], VALID_SOURCE),
    Twin::new(STARTS[29], VALID),
    Twin::new(STARTS[30], VALID),
];

/// The settings of the twin check.
const TWINS: Twins<'_> = Twins::new(EXAMPLES, &[codes::ALL])
    .pairs(PAIRS)
    .unprovoked(UNPROVOKED);

/// Everything the checks read, which `conventions!` expands to one test for
/// each check. `Suite::verbs` sets the verbs accepted under a resource, and
/// `usage` is the one that `self usage` adds. The budgets of the `-h` pages are crumbkit's defaults.
const SUITE: Suite<'_> = Suite::new(|| (Cli::command(), TOOL))
    .allow(ALLOW)
    .verbs(&["usage"])
    .path_budgets(PATH_BUDGETS)
    .examples(EXAMPLES)
    .recoveries(RECOVERIES)
    .twins(TWINS);

crumbkit_test::conventions!(SUITE);
