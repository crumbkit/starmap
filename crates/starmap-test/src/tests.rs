//! Tests for the struct test, with a golden file of each disagreement
//! report.

use std::path::PathBuf;

use snapbox::assert::DEFAULT_ACTION_ENV;
use snapbox::{Assert, Data};

use super::{Allow, check, check_allowing, compare};
use starmap::Env;

/// The illustrative `starmap.toml` of the build plan.
const EXAMPLE: &str = include_str!("../../starmap/tests/example/starmap.toml");

/// A spec that disagrees with [`EveryKind`] in each way the check reports.
const EVERY_KIND: &str = include_str!("../tests/golden/every_kind.toml");

/// The example struct of the build plan, which agrees with its spec.
#[derive(starmap::Env)]
#[starmap(spec = "../starmap/tests/example/starmap.toml")]
struct Shelf {
    #[starmap("SHELF_ENV")]
    _stage: Stage,
    #[starmap("SHELF_PORT")]
    _port: starmap::Port,
    #[starmap("SHELF_CATALOGUE_URL")]
    _catalogue: Option<url::Url>,
    #[starmap("SHELF_DATABASE")]
    _database: PathBuf,
    #[starmap("SHELF_DEBUG")]
    _debug: bool,
    #[starmap("SHELF_PAGE_SIZE")]
    _page_size: u8,
    #[starmap("SHELF_MIRROR_TOKEN")]
    _mirror_token: Option<starmap::Secret<String>>,
    #[starmap("SHELF_SESSION_KEY")]
    _session_key: starmap::Secret<String>,
    #[starmap("XDG_STATE_HOME")]
    _state_home: Option<PathBuf>,
}

#[derive(starmap::Enumeration)]
enum Stage {
    Dev,
    Prod,
}

/// The struct of the build plan's example report: its mirror token is not
/// a secret, and it reads SHELF_THEME, which the spec does not declare.
#[derive(starmap::Env)]
#[starmap(spec = "../starmap/tests/example/starmap.toml")]
struct Stale {
    #[starmap("SHELF_ENV")]
    _stage: Stage,
    #[starmap("SHELF_PORT")]
    _port: starmap::Port,
    #[starmap("SHELF_CATALOGUE_URL")]
    _catalogue: Option<url::Url>,
    #[starmap("SHELF_DATABASE")]
    _database: PathBuf,
    #[starmap("SHELF_DEBUG")]
    _debug: bool,
    #[starmap("SHELF_PAGE_SIZE")]
    _page_size: u8,
    #[starmap("SHELF_MIRROR_TOKEN")]
    _mirror_token: Option<String>,
    #[starmap("SHELF_SESSION_KEY")]
    _session_key: starmap::Secret<String>,
    #[starmap("XDG_STATE_HOME")]
    _state_home: Option<PathBuf>,
    #[starmap("SHELF_THEME")]
    _theme: Option<String>,
}

/// A struct that disagrees with [`EVERY_KIND`] in each way the check
/// reports.
#[derive(starmap::Env)]
#[starmap(spec = "tests/golden/every_kind.toml")]
struct EveryKind {
    #[starmap("SHELF_ENV")]
    _stage: Stage,
    #[starmap("SHELF_PORT")]
    _port: PathBuf,
    #[starmap("SHELF_PAGE_SIZE")]
    _page_size: u8,
    #[starmap("SHELF_TOKEN")]
    _token: String,
    #[starmap("SHELF_COLOUR")]
    _colour: starmap::Secret<String>,
    #[starmap("SHELF_MIRROR_TOKEN")]
    _mirror_token: starmap::Secret<String>,
    #[starmap("SHELF_DEBUG")]
    _debug: Option<bool>,
    #[starmap("SHELF_THEME")]
    _theme: String,
}

/// The Allow entries of [`EveryKind`]: one that excuses SHELF_HELPER, and
/// two that excuse nothing.
const EVERY_KIND_ALLOW: &[Allow] = &[
    Allow {
        name: "SHELF_HELPER",
        reason: "only a dependency reads it",
    },
    Allow {
        name: "SHELF_PORT",
        reason: "the server reads it",
    },
    Allow {
        name: "SHELF_GONE",
        reason: "a dependency once read it",
    },
];

/// Returns the report of `E` against `spec`, which must disagree.
fn report<E: Env>(spec: &str, allow: &[Allow]) -> String {
    compare(std::any::type_name::<E>(), E::FIELDS, spec, allow).unwrap_err()
}

/// Compares `actual` with the golden file `name` under `tests/golden`, or
/// rewrites the file when `SNAPSHOTS=overwrite` is set.
fn golden(actual: String, name: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    Assert::new()
        .action_env(DEFAULT_ACTION_ENV)
        .eq(Data::text(actual + "\n"), Data::read_from(&path, None));
}

#[test]
fn the_example_struct_agrees_with_its_spec() {
    check::<Shelf>(EXAMPLE);
}

#[test]
fn the_report_of_the_plan_example_names_each_disagreement() {
    let spec = EXAMPLE.replace("max = 100 }", "max = 1000 }");
    golden(report::<Stale>(&spec, &[]), "plan_example.txt");
}

#[test]
fn the_report_lists_every_kind_of_disagreement_at_once() {
    golden(
        report::<EveryKind>(EVERY_KIND, EVERY_KIND_ALLOW),
        "every_kind.txt",
    );
}

#[test]
fn an_allow_entry_excuses_a_declared_variable_the_struct_does_not_read() {
    let spec = format!(
        "{EXAMPLE}\n{}",
        r#"
[variables.XDG_CONFIG_HOME]
purpose = "The directory that holds a dependency's configuration."
type = { path = { absolute = true } }
unset = "optional"
show = "shown"
source = "inherited"
"#
    );
    let allow = [Allow {
        name: "XDG_CONFIG_HOME",
        reason: "only a dependency reads it",
    }];
    assert_eq!(compare("Shelf", Shelf::FIELDS, &spec, &allow), Ok(()));
}

#[test]
#[should_panic(expected = "disagree in 13 places:\n  SHELF_COLOUR        starmap.toml shows it")]
fn check_panics_with_the_report() {
    check_allowing::<EveryKind>(EVERY_KIND, EVERY_KIND_ALLOW);
}

#[test]
#[should_panic(
    expected = "the spec text the test passes differs from the starmap.toml that starmap_test::tests::Shelf embeds"
)]
fn check_panics_when_the_test_passes_another_spec_than_the_struct_embeds() {
    check::<Shelf>(EVERY_KIND);
}

#[test]
fn without_a_mode_table_a_production_rule_never_leaves_a_variable_unset() {
    let spec = EXAMPLE
        .replace(
            "[mode]\nvariable = \"SHELF_ENV\"\nproduction = [\"prod\"]\n",
            "",
        )
        .replace(
            r#"unset = { local = "optional", production = "required" }"#,
            r#"unset = { local = "required", production = "optional" }"#,
        );
    let expected = "SHELF_MIRROR_TOKEN  starmap.toml always gives it a value";
    let actual = report::<Shelf>(&spec, &[]);
    assert!(actual.contains(expected), "{actual}");
    assert!(actual.contains("disagree in 1 place:"), "{actual}");
}

#[test]
fn a_spec_that_does_not_parse_is_reported() {
    let actual = report::<Shelf>("[variables", &[]);
    assert!(
        actual.starts_with("starmap.toml does not parse: "),
        "{actual}"
    );
}
