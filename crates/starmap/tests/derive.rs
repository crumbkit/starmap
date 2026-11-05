//! The behaviour of `#[derive(Env)]` and `#[derive(Enumeration)]` through
//! starmap's `derive` feature, on the example struct `ShelfEnv`.
#![cfg(feature = "derive")]

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;

use starmap::{Enumeration, Env, Port, Presence, ReadError, Secret, Shape, Show};

/// The example struct `ShelfEnv`, over the example spec in
/// `tests/example/starmap.toml`.
#[derive(Debug, starmap::Env)]
#[starmap(spec = "tests/example/starmap.toml")]
struct ShelfEnv {
    #[starmap("SHELF_ENV")]
    stage: Stage,
    #[starmap("SHELF_PORT")]
    port: starmap::Port,
    #[starmap("SHELF_CATALOGUE_URL")]
    catalogue: Option<url::Url>,
    #[starmap("SHELF_DATABASE")]
    database: PathBuf,
    #[starmap("SHELF_DEBUG")]
    debug: bool,
    #[starmap("SHELF_PAGE_SIZE")]
    page_size: u8,
    #[starmap("SHELF_MIRROR_TOKEN")]
    mirror_token: Option<starmap::Secret>,
    #[starmap("SHELF_SESSION_KEY")]
    session_key: starmap::Secret,
    #[starmap("XDG_STATE_HOME")]
    state_home: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq, starmap::Enumeration)]
enum Stage {
    Dev,
    Prod,
}

/// An enumeration whose variants read as kebab case, and one renamed.
#[derive(Debug, PartialEq, Eq, starmap::Enumeration)]
enum Speed {
    DryRun,
    HTTPServer,
    #[starmap("full")]
    Flat,
}

/// Returns `pairs` as the variables of an environment.
fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

#[test]
fn the_derive_embeds_the_spec_that_the_struct_names() {
    assert_eq!(ShelfEnv::SPEC, include_str!("example/starmap.toml"));
}

#[test]
fn each_field_type_sets_the_shape_presence_and_show_of_its_field() {
    let fields: Vec<_> = ShelfEnv::FIELDS
        .iter()
        .map(|field| {
            (
                field.name,
                field.rust_type,
                field.shape,
                field.presence,
                field.show,
            )
        })
        .collect();
    assert_eq!(
        fields,
        [
            (
                "SHELF_ENV",
                "Stage",
                Shape::Enumeration(&["dev", "prod"]),
                Presence::Always,
                Show::Shown
            ),
            (
                "SHELF_PORT",
                "starmap::Port",
                Shape::Port,
                Presence::Always,
                Show::Shown
            ),
            (
                "SHELF_CATALOGUE_URL",
                "url::Url",
                Shape::Url,
                Presence::Maybe,
                Show::Shown
            ),
            (
                "SHELF_DATABASE",
                "PathBuf",
                Shape::Path,
                Presence::Always,
                Show::Shown
            ),
            (
                "SHELF_DEBUG",
                "bool",
                Shape::Boolean,
                Presence::Always,
                Show::Shown
            ),
            (
                "SHELF_PAGE_SIZE",
                "u8",
                Shape::Integer { min: 0, max: 255 },
                Presence::Always,
                Show::Shown
            ),
            (
                "SHELF_MIRROR_TOKEN",
                "starmap::Secret",
                Shape::String,
                Presence::Maybe,
                Show::Withheld
            ),
            (
                "SHELF_SESSION_KEY",
                "starmap::Secret",
                Shape::String,
                Presence::Always,
                Show::Withheld
            ),
            (
                "XDG_STATE_HOME",
                "PathBuf",
                Shape::Path,
                Presence::Maybe,
                Show::Shown
            ),
        ]
    );
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test checks the secrets that the struct reads"
)]
fn the_example_struct_loads_from_a_fixed_set_of_variables() {
    let env = ShelfEnv::from_vars(vars(&[
        ("SHELF_PORT", "8080"),
        ("SHELF_CATALOGUE_URL", "https://shelf.example.org/"),
        ("SHELF_DEBUG", "yes"),
        ("SHELF_MIRROR_TOKEN", "mirror-token"),
        ("SHELF_SESSION_KEY", "session-key"),
        ("XDG_STATE_HOME", "relative/state"),
        ("UNDECLARED", "ignored"),
    ]))
    .expect("the variables fit the spec");

    assert_eq!(env.stage, Stage::Dev);
    assert_eq!(env.port, Port::try_from(8080).expect("8080 is a port"));
    assert_eq!(
        env.catalogue.map(String::from).as_deref(),
        Some("https://shelf.example.org/")
    );
    assert_eq!(env.database, PathBuf::from("books.db"));
    assert!(env.debug);
    assert_eq!(env.page_size, 20);
    assert_eq!(
        env.mirror_token.as_ref().map(Secret::expose),
        Some("mirror-token")
    );
    assert_eq!(env.session_key.expose(), "session-key");
    assert_eq!(
        env.state_home, None,
        "a relative path counts as unset where absolute = true"
    );
}

#[test]
fn the_mode_variable_decides_the_mode_the_struct_is_read_in() {
    let error = ShelfEnv::from_vars(vars(&[
        ("SHELF_ENV", "prod"),
        ("SHELF_PORT", "8080"),
        ("SHELF_SESSION_KEY", "session-key"),
    ]))
    .expect_err("the mirror token is required in production");
    assert_eq!(
        error.to_string(),
        "SHELF_MIRROR_TOKEN is required in production mode and has no value"
    );
}

#[test]
fn a_missing_or_invalid_variable_refuses_the_read_with_every_finding() {
    let error = ShelfEnv::from_vars(vars(&[("SHELF_PAGE_SIZE", "500")]))
        .expect_err("the port is missing and the page size is out of range");
    assert!(matches!(error, ReadError::Refused(_)));
    assert_eq!(
        error.to_string(),
        "SHELF_PORT is required in local mode and has no value; \
         SHELF_PAGE_SIZE: the value is not a whole number from 1 to 100; \
         SHELF_SESSION_KEY is required in local mode and has no value"
    );
}

/// A struct whose page size field is narrower than its spec's range.
#[derive(Debug, starmap::Env)]
#[starmap(spec = "tests/example/narrow.toml")]
struct Narrow {
    #[starmap("SHELF_PAGE_SIZE")]
    #[expect(dead_code, reason = "the test reads the struct only to see it refused")]
    page_size: u8,
}

#[test]
fn a_field_that_cannot_hold_a_checked_value_refuses_the_read() {
    let error =
        Narrow::from_vars(vars(&[("SHELF_PAGE_SIZE", "300")])).expect_err("a u8 cannot hold 300");
    assert!(matches!(error, ReadError::Field(ref field) if field.name() == "SHELF_PAGE_SIZE"));
    assert_eq!(
        error.to_string(),
        "the struct cannot read SHELF_PAGE_SIZE into its field"
    );
    assert_eq!(
        std::error::Error::source(&error)
            .map(ToString::to_string)
            .as_deref(),
        Some(
            "the field that reads SHELF_PAGE_SIZE cannot hold its checked value, because the struct and starmap.toml disagree"
        )
    );
}

#[test]
fn an_enumeration_variant_reads_as_its_kebab_case_name_with_a_run_of_capitals_as_one_word_or_its_rename()
 {
    assert_eq!(Speed::VALUES, ["dry-run", "http-server", "full"]);
    let choice = |value: &str| value.parse::<starmap::Choice>().expect("a choice");
    assert_eq!(Speed::from_choice(&choice("dry-run")), Some(Speed::DryRun));
    assert_eq!(Speed::from_choice(&choice("full")), Some(Speed::Flat));
    assert_eq!(Speed::from_choice(&choice("flat")), None);
}

#[test]
fn an_enumeration_variant_returns_its_value() {
    assert_eq!(Speed::DryRun.value(), "dry-run");
    assert_eq!(Speed::HTTPServer.value(), "http-server");
    assert_eq!(Speed::Flat.value(), "full");
}

/// A struct over a spec without a `[mode]` table, which reads a socket
/// address.
#[derive(Debug, starmap::Env)]
#[starmap(spec = "tests/example/listen.toml")]
struct Listen {
    #[starmap("SHELF_LISTEN_ADDRESS")]
    address: SocketAddr,
    #[starmap("SHELF_STAGE")]
    stage: Stage,
}

#[test]
fn a_socket_address_field_reads_its_value_or_its_default() {
    let set = Listen::from_vars(vars(&[
        ("SHELF_LISTEN_ADDRESS", "[::1]:9090"),
        ("SHELF_STAGE", "prod"),
    ]))
    .expect("the variables fit the spec");
    let defaulted =
        Listen::from_vars(vars(&[("SHELF_STAGE", "dev")])).expect("the address has a default");

    assert_eq!(
        set.address,
        SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], 9090))
    );
    assert_eq!(set.stage, Stage::Prod);
    assert_eq!(defaulted.address, SocketAddr::from(([0, 0, 0, 0], 8080)));
}

#[test]
fn without_a_mode_table_a_refusal_names_no_mode_and_no_file() {
    let missing = Listen::from_vars(vars(&[])).expect_err("the stage is required");
    let empty = Listen::from_vars(vars(&[
        ("SHELF_LISTEN_ADDRESS", "localhost:8080"),
        ("SHELF_STAGE", ""),
    ]))
    .expect_err("the address names a host and the stage is empty");

    assert_eq!(
        missing.to_string(),
        "SHELF_STAGE is required and has no value"
    );
    assert_eq!(
        empty.to_string(),
        "SHELF_LISTEN_ADDRESS: the value is not an ip socket address, such as 127.0.0.1:8080 or [::1]:8080; \
         SHELF_STAGE: the value is empty, and it must be one of dev, prod"
    );
}

#[test]
fn unsupported_field_types_and_misuses_fail_to_compile_with_a_clear_message() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/*.rs");
}

/// A struct whose embedded spec is not TOML.
#[derive(Debug, starmap::Env)]
#[starmap(spec = "tests/spec/syntax.toml")]
struct Broken {}

#[test]
fn an_embedded_spec_that_does_not_parse_refuses_the_read() {
    let error = Broken::from_vars(vars(&[])).expect_err("the spec is not TOML");
    assert!(matches!(error, ReadError::Spec(_)));
    assert_eq!(error.to_string(), "the embedded starmap.toml is invalid");
}
