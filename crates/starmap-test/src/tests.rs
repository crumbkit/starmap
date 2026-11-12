//! Tests for the struct test, with a golden file of each disagreement
//! report.

use std::path::PathBuf;

use snapbox::assert::DEFAULT_ACTION_ENV;
use snapbox::{Assert, Data};

use super::{Allow, Reader, check, check_allowing, check_together, compare, compare_together};
use starmap::Env;

/// The example spec in `crates/starmap/tests/example/starmap.toml`.
const EXAMPLE: &str = include_str!("../../starmap/tests/example/starmap.toml");

/// A spec that disagrees with [`EveryKind`] in each way the check reports.
const EVERY_KIND: &str = include_str!("../tests/golden/every_kind.toml");

/// A copy of the example struct `ShelfEnv` of
/// `crates/starmap/tests/derive.rs`, which agrees with its spec.
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
    _mirror_token: Option<starmap::Secret>,
    #[starmap("SHELF_SESSION_KEY")]
    _session_key: starmap::Secret,
    #[starmap("XDG_STATE_HOME")]
    _state_home: Option<PathBuf>,
}

#[derive(starmap::Enumeration)]
enum Stage {
    Dev,
    Prod,
}

/// A stale copy of the example struct: its mirror token is not a secret,
/// and it reads SHELF_THEME, which the spec does not declare.
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
    _session_key: starmap::Secret,
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
    _colour: starmap::Secret,
    #[starmap("SHELF_MIRROR_TOKEN")]
    _mirror_token: starmap::Secret,
    #[starmap("SHELF_DEBUG")]
    _debug: Option<bool>,
    #[starmap("SHELF_THEME")]
    _theme: String,
}

/// The Allow entries of [`EveryKind`]: one that excuses SHELF_HELPER, and
/// two that excuse nothing.
const EVERY_KIND_ALLOW: &[Allow] = &[
    Allow::new("SHELF_HELPER", "only a dependency reads it"),
    Allow::new("SHELF_PORT", "the server reads it"),
    Allow::new("SHELF_GONE", "a dependency once read it"),
];

/// Returns the report of `E` against `spec`, which must disagree.
fn report<E: Env>(spec: &str, allow: &[Allow]) -> String {
    compare(
        std::any::type_name::<E>(),
        "starmap.toml",
        E::FIELDS,
        spec,
        allow,
    )
    .unwrap_err()
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
    check::<Shelf>();
}

#[test]
fn the_report_of_a_stale_copy_of_the_example_names_each_disagreement() {
    let spec = EXAMPLE.replace("max = 100 }", "max = 1000 }");
    golden(report::<Stale>(&spec, &[]), "stale_example.txt");
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
    let allow = [Allow::new("XDG_CONFIG_HOME", "only a dependency reads it")];
    assert_eq!(
        compare("Shelf", "starmap.toml", Shelf::FIELDS, &spec, &allow),
        Ok(())
    );
}

#[test]
#[should_panic(
    expected = "disagree in 13 places:\n  SHELF_COLOUR        tests/golden/every_kind.toml shows it"
)]
fn check_panics_with_the_report() {
    check_allowing::<EveryKind>(EVERY_KIND_ALLOW);
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

/// The spec of [`Listen`], which reads a socket address.
const LISTEN: &str = include_str!("../../starmap/tests/example/listen.toml");

/// A struct that agrees with [`LISTEN`].
#[derive(starmap::Env)]
#[starmap(spec = "../starmap/tests/example/listen.toml")]
struct Listen {
    #[starmap("SHELF_LISTEN_ADDRESS")]
    _address: std::net::SocketAddr,
    #[starmap("SHELF_STAGE")]
    _stage: Stage,
}

/// A struct that reads [`LISTEN`]'s socket address as a port.
#[derive(starmap::Env)]
#[starmap(spec = "../starmap/tests/example/listen.toml")]
struct ListenPort {
    #[starmap("SHELF_LISTEN_ADDRESS")]
    _address: starmap::Port,
    #[starmap("SHELF_STAGE")]
    _stage: Stage,
}

#[test]
fn a_socket_address_field_agrees_with_a_socket_address_variable() {
    check::<Listen>();
}

#[test]
fn a_port_field_disagrees_with_a_socket_address_variable() {
    let actual = report::<ListenPort>(LISTEN, &[]);
    assert!(
        actual.contains(
            "SHELF_LISTEN_ADDRESS  starmap.toml types it as an ip socket address, and the field type starmap::Port holds a port"
        ),
        "{actual}"
    );
}

#[test]
fn a_string_or_secret_field_agrees_with_a_string_that_has_a_pattern() {
    let spec = r#"
[variables.SHELF_REVISION]
purpose = "Which commit of the catalogue shelf serves."
type = { string = { pattern = "[0-9a-f]{40}" } }
unset = "required"
show = "shown"
source = "setting"

[variables.SHELF_MIRROR_KEY]
purpose = "The key shelf calls the catalogue mirror with."
type = { string = { pattern = "sk-[a-z0-9]+" } }
unset = "required"
show = "withheld"
source = "secret"
"#;
    let fields = [
        starmap::Field::new::<String>("SHELF_REVISION", "String", starmap::Presence::Always),
        starmap::Field::new::<starmap::Secret>(
            "SHELF_MIRROR_KEY",
            "starmap::Secret",
            starmap::Presence::Always,
        ),
    ];

    assert_eq!(compare("Shelf", "starmap.toml", &fields, spec, &[]), Ok(()));
}

/// The spec of [`Keys`], which reads a withheld and a shown bytes value.
const KEYS: &str = include_str!("../../starmap/tests/example/keys.toml");

/// A struct that agrees with [`KEYS`].
#[derive(starmap::Env)]
#[starmap(spec = "../starmap/tests/example/keys.toml")]
struct Keys {
    #[starmap("SHELF_COOKIE_KEY")]
    _cookie_key: starmap::SecretBytes,
    #[starmap("SHELF_SALT")]
    _salt: Vec<u8>,
}

/// A struct that reads [`KEYS`]'s withheld key as text and its salt as a
/// string.
#[derive(starmap::Env)]
#[starmap(spec = "../starmap/tests/example/keys.toml")]
struct KeysAsText {
    #[starmap("SHELF_COOKIE_KEY")]
    _cookie_key: starmap::Secret,
    #[starmap("SHELF_SALT")]
    _salt: String,
}

#[test]
fn bytes_fields_agree_with_bytes_variables() {
    check::<Keys>();
}

#[test]
fn text_fields_disagree_with_bytes_variables() {
    let actual = report::<KeysAsText>(KEYS, &[]);
    for expected in [
        "SHELF_COOKIE_KEY  starmap.toml types it as 4 bytes as base64, and the field type starmap::Secret holds a string",
        "SHELF_SALT        starmap.toml types it as at least 2 bytes as hex, and the field type String holds a string",
    ] {
        assert!(actual.contains(expected), "{actual}");
    }
}

#[test]
fn a_url_field_agrees_with_a_url_that_has_schemes() {
    let spec = r#"
[variables.SHELF_DATABASE_URL]
purpose = "The database shelf keeps its catalogue in."
type = { url = { schemes = ["postgres", "postgresql"] } }
unset = "required"
show = "shown"
source = "setting"
"#;
    let fields = [starmap::Field::new::<url::Url>(
        "SHELF_DATABASE_URL",
        "url::Url",
        starmap::Presence::Always,
    )];

    assert_eq!(compare("Shelf", "starmap.toml", &fields, spec, &[]), Ok(()));
}

#[test]
fn a_bool_field_agrees_with_an_optional_variable_that_reads_its_presence_and_an_option_does_not() {
    let spec = include_str!("../../starmap/tests/example/colour.toml");
    let field = |presence| [starmap::Field::new::<bool>("NO_COLOR", "bool", presence)];

    assert_eq!(
        compare(
            "Shelf",
            "starmap.toml",
            &field(starmap::Presence::Always),
            spec,
            &[]
        ),
        Ok(())
    );
    let report = compare(
        "Shelf",
        "starmap.toml",
        &field(starmap::Presence::Maybe),
        spec,
        &[],
    )
    .unwrap_err();
    assert!(
        report.contains(
            "NO_COLOR  starmap.toml always gives it a value, and the field type Option<bool> \
             holds an unset value"
        ),
        "{report}"
    );
}

/// The spec of families in `crates/starmap/tests/example/regions.toml`.
const REGIONS: &str = include_str!("../../starmap/tests/example/regions.toml");

/// A struct that reads every variable and family of [`REGIONS`].
#[derive(starmap::Env)]
#[starmap(spec = "../starmap/tests/example/regions.toml")]
struct Regions {
    #[starmap("SHELF_STAGE")]
    _stage: Stage,
    #[starmap(family = "SHELF_DB_URL")]
    _databases: starmap::Family<url::Url>,
    #[starmap(family = "SHELF_TOKEN")]
    _tokens: starmap::Family<starmap::Secret>,
}

/// A struct that reads the database family as text and skips the tokens.
#[derive(starmap::Env)]
#[starmap(spec = "../starmap/tests/example/regions.toml")]
struct WrongRegions {
    #[starmap("SHELF_STAGE")]
    _stage: Stage,
    #[starmap(family = "SHELF_DB_URL")]
    _databases: starmap::Family<bool>,
    #[starmap(family = "SHELF_TENANT")]
    _tenants: starmap::Family<String>,
}

#[test]
fn a_family_field_that_matches_its_family_agrees() {
    check::<Regions>();
}

#[test]
fn a_family_field_of_the_wrong_type_an_undeclared_family_and_an_unread_family_are_reported() {
    let actual = report::<WrongRegions>(REGIONS, &[]);

    assert!(actual.contains("disagree in 3 places:"), "{actual}");
    assert!(
        actual.contains(
            "SHELF_DB_URL  starmap.toml types it as a url, and the field type bool holds a boolean"
        ),
        "{actual}"
    );
    assert!(
        actual.contains(
            "SHELF_TENANT  the struct reads it as a family, and starmap.toml declares no family with that prefix"
        ),
        "{actual}"
    );
    assert!(
        actual.contains(
            "SHELF_TOKEN   starmap.toml declares the family, and the struct neither reads it nor has an Allow entry for it"
        ),
        "{actual}"
    );
}

#[test]
fn an_allow_entry_excuses_a_family_the_struct_does_not_read() {
    let fields = &Regions::FIELDS[..2];
    let allow = [Allow::new("SHELF_TOKEN", "only a dependency reads it")];

    assert_eq!(
        compare("Regions", "starmap.toml", fields, REGIONS, &allow),
        Ok(())
    );
}

/// The serve command's struct over the example spec, which reads the
/// stage, the port, the database and both secrets.
#[derive(starmap::Env)]
#[starmap(spec = "../starmap/tests/example/starmap.toml")]
struct Serve {
    #[starmap("SHELF_ENV")]
    _stage: Stage,
    #[starmap("SHELF_PORT")]
    _port: starmap::Port,
    #[starmap("SHELF_DATABASE")]
    _database: PathBuf,
    #[starmap("SHELF_MIRROR_TOKEN")]
    _mirror_token: Option<starmap::Secret>,
    #[starmap("SHELF_SESSION_KEY")]
    _session_key: starmap::Secret,
}

/// The api command's struct over the example spec, which reads the port
/// and every variable that [`Serve`] does not read.
#[derive(starmap::Env)]
#[starmap(spec = "../starmap/tests/example/starmap.toml")]
struct Api {
    #[starmap("SHELF_PORT")]
    _port: starmap::Port,
    #[starmap("SHELF_CATALOGUE_URL")]
    _catalogue: Option<url::Url>,
    #[starmap("SHELF_DEBUG")]
    _debug: bool,
    #[starmap("SHELF_PAGE_SIZE")]
    _page_size: u8,
    #[starmap("XDG_STATE_HOME")]
    _state_home: Option<PathBuf>,
}

/// A stale api struct over the example spec: it reads the debug flag as
/// text, and leaves the catalogue url, the page size and the state home
/// unread.
#[derive(starmap::Env)]
#[starmap(spec = "../starmap/tests/example/starmap.toml")]
struct StaleApi {
    #[starmap("SHELF_PORT")]
    _port: starmap::Port,
    #[starmap("SHELF_DEBUG")]
    _debug: String,
}

#[test]
fn structs_that_together_read_every_declared_variable_agree_with_their_spec() {
    check_together(&[Reader::of::<Serve>(), Reader::of::<Api>()], &[]);
}

#[test]
fn the_report_of_several_structs_names_the_struct_of_each_field_and_each_variable_none_reads() {
    let allow = [
        Allow::new("SHELF_PAGE_SIZE", "only a dependency reads it"),
        Allow::new("SHELF_PORT", "the server reads it"),
    ];
    let structs = [
        (std::any::type_name::<Serve>(), Serve::FIELDS),
        (std::any::type_name::<StaleApi>(), StaleApi::FIELDS),
    ];
    let actual = compare_together(&structs, "starmap.toml", EXAMPLE, &allow).unwrap_err();

    golden(actual, "together.txt");
}

#[test]
#[should_panic(
    expected = "check_together checks structs that embed one file, and starmap_test::tests::Listen embeds ../starmap/tests/example/listen.toml, \
                which is not the file that starmap_test::tests::Serve embeds"
)]
fn check_together_refuses_structs_that_embed_different_files() {
    check_together(&[Reader::of::<Serve>(), Reader::of::<Listen>()], &[]);
}

#[test]
#[should_panic(expected = "check_together needs at least one struct")]
fn check_together_refuses_an_empty_list_of_structs() {
    check_together(&[], &[]);
}
