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
fn the_derive_names_the_file_it_embeds() {
    assert_eq!(ShelfEnv::SPEC_FILE, "tests/example/starmap.toml");
}

/// A struct over a pointer to the example spec.
#[derive(Debug, starmap::Env)]
#[starmap(spec = "tests/pointer/starmap.toml")]
struct PointedEnv {}

#[test]
fn the_derive_follows_a_pointer_and_embeds_the_file_it_names() {
    assert_eq!(PointedEnv::SPEC, include_str!("example/starmap.toml"));
    assert_eq!(PointedEnv::SPEC_FILE, "tests/example/starmap.toml");
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
fn an_empty_value_reads_as_none_for_an_optional_field_and_as_its_default_for_a_defaulted_one() {
    let env = ShelfEnv::from_vars(vars(&[
        ("SHELF_ENV", ""),
        ("SHELF_PORT", "8080"),
        ("SHELF_CATALOGUE_URL", ""),
        ("SHELF_PAGE_SIZE", ""),
        ("SHELF_MIRROR_TOKEN", ""),
        ("SHELF_SESSION_KEY", "session-key"),
    ]))
    .expect("an empty value counts as unset");

    assert_eq!(env.stage, Stage::Dev);
    assert_eq!(env.catalogue, None);
    assert_eq!(env.page_size, 20);
    assert!(env.mirror_token.is_none());
}

#[test]
fn an_empty_value_refuses_the_read_of_a_required_field() {
    let error = ShelfEnv::from_vars(vars(&[("SHELF_PORT", "8080"), ("SHELF_SESSION_KEY", "")]))
        .expect_err("the session key is required");

    assert!(matches!(error, ReadError::Refused(_)));
    assert_eq!(
        error.to_string(),
        "SHELF_SESSION_KEY is required in local mode and has no value"
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
         SHELF_PAGE_SIZE: the value is not a whole number from 1 to 100 (the value is \"500\"); \
         SHELF_SESSION_KEY is required in local mode and has no value"
    );
}

#[cfg(unix)]
#[test]
fn a_withheld_value_that_does_not_fit_refuses_the_read_without_a_quote() {
    use std::os::unix::ffi::OsStringExt as _;

    let error = ShelfEnv::from_vars([
        (OsString::from("SHELF_PORT"), OsString::from("8080")),
        (
            OsString::from("SHELF_SESSION_KEY"),
            OsString::from_vec(b"session-\xffkey".to_vec()),
        ),
    ])
    .expect_err("the session key is not unicode text");

    assert_eq!(
        error.to_string(),
        "SHELF_SESSION_KEY: the value is not unicode text"
    );
    assert!(!format!("{error:?}").contains("session-"));
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

/// A struct with a shown string field and a required port.
#[derive(Debug, starmap::Env)]
#[starmap(spec = "tests/example/title.toml")]
#[expect(dead_code, reason = "the test reads the struct only to see it refused")]
struct Titled {
    #[starmap("SHELF_TITLE")]
    title: String,
    #[starmap("SHELF_PORT")]
    port: Port,
}

#[test]
#[cfg(unix)]
fn a_string_value_that_is_not_unicode_and_a_missing_variable_are_refused_together() {
    use std::os::unix::ffi::OsStringExt as _;

    let title = OsString::from_vec(vec![b'S', 0xff]);
    let error = Titled::from_vars([(OsString::from("SHELF_TITLE"), title)])
        .expect_err("the title is not unicode and the port is missing");

    assert!(matches!(error, ReadError::Refused(_)));
    assert_eq!(
        error.to_string(),
        "SHELF_TITLE: the value is not unicode text (the value is \"S\u{fffd}\"); \
         SHELF_PORT is required and has no value"
    );
}

/// A struct with a field that reads a variable's presence.
#[derive(Debug, starmap::Env)]
#[starmap(spec = "tests/example/colour.toml")]
struct Colour {
    #[starmap("NO_COLOR")]
    no_colour: bool,
}

#[test]
fn a_bool_field_of_a_presence_variable_is_true_for_any_value_and_false_when_it_is_absent() {
    for set in ["", "1", "false"] {
        let env = Colour::from_vars(vars(&[("NO_COLOR", set)])).expect("any value sets it");
        assert!(env.no_colour, "{set:?}");
    }

    let env = Colour::from_vars(vars(&[])).expect("the variable may be absent");
    assert!(!env.no_colour);
}

/// A struct over a spec whose variables count an empty value as a value,
/// with an application check on the greeting.
#[derive(Debug, starmap::Env)]
#[starmap(spec = "tests/example/empty.toml")]
struct Workers {
    #[starmap("SHELF_WORKERS")]
    workers: u8,
    #[starmap("SHELF_GREETING", check = not_blank)]
    greeting: String,
}

/// Refuses a greeting that is empty or only white space.
#[expect(
    clippy::ptr_arg,
    reason = "a check takes a reference to the field's type, which is String"
)]
fn not_blank(greeting: &String) -> Result<(), String> {
    if greeting.trim().is_empty() {
        Err("the greeting is not blank".to_owned())
    } else {
        Ok(())
    }
}

#[test]
fn an_empty_value_reaches_the_apps_check_when_the_variable_declares_empty_a_value() {
    let error = Workers::from_vars(vars(&[("SHELF_WORKERS", ""), ("SHELF_GREETING", "")]))
        .expect_err("an empty count is not a whole number and an empty greeting is blank");

    let ReadError::Refused(refusals) = &error else {
        panic!("the empty values refuse the read, and gave {error:?}");
    };
    let problems: Vec<(&str, &starmap::Problem)> = refusals
        .iter()
        .map(|finding| (finding.name.as_str(), &finding.problem))
        .collect();
    assert_eq!(
        problems,
        [
            (
                "SHELF_WORKERS",
                &starmap::Problem::Invalid(starmap::TypeError::NotInteger)
            ),
            (
                "SHELF_GREETING",
                &starmap::Problem::Rejected {
                    reason: "the greeting is not blank".to_owned()
                }
            ),
        ]
    );
    assert_eq!(
        error.to_string(),
        "SHELF_WORKERS: the value is not a whole number (the value is \"\"); \
         SHELF_GREETING: the greeting is not blank (the value is \"\")"
    );
}

#[test]
fn an_absent_variable_that_declares_empty_a_value_takes_its_default() {
    let env = Workers::from_vars(vars(&[])).expect("both variables have defaults");

    assert_eq!(env.workers, 5);
    assert_eq!(env.greeting, "Welcome");
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

/// A struct over a spec of bytes variables.
#[derive(Debug, starmap::Env)]
#[starmap(spec = "tests/example/keys.toml")]
struct Keys {
    #[starmap("SHELF_COOKIE_KEY")]
    cookie_key: starmap::SecretBytes,
    #[starmap("SHELF_SALT")]
    salt: Vec<u8>,
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test checks the bytes that the withheld field holds"
)]
fn the_derive_gives_a_shown_bytes_field_vec_u8_and_a_withheld_one_the_decoded_bytes() {
    let keys = Keys::from_vars(vars(&[("SHELF_COOKIE_KEY", "3q2+7w==")]))
        .expect("the variables fit the spec");

    assert_eq!(keys.cookie_key.expose(), [0xde, 0xad, 0xbe, 0xef]);
    assert_eq!(keys.salt, [0xbe, 0xef]);
    assert_eq!(
        Keys::FIELDS
            .iter()
            .map(|field| (field.shape, field.show))
            .collect::<Vec<_>>(),
        [(Shape::Bytes, Show::Withheld), (Shape::Bytes, Show::Shown)]
    );
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
        "SHELF_LISTEN_ADDRESS: the value is not an ip socket address, such as 127.0.0.1:8080 or [::1]:8080 \
         (the value is \"localhost:8080\"); SHELF_STAGE is required and has no value"
    );
}

/// A struct over the example spec whose fields name the application's own
/// checks.
#[derive(Debug, starmap::Env)]
#[starmap(spec = "tests/example/starmap.toml")]
struct CheckedShelf {
    #[starmap("SHELF_PORT", check = unprivileged)]
    port: starmap::Port,
    #[starmap("SHELF_CATALOGUE_URL", check = https)]
    catalogue: Option<url::Url>,
    #[starmap("SHELF_SESSION_KEY", check = long_key)]
    session_key: starmap::Secret,
}

/// Refuses a port of 1024 or below, naming the port.
fn unprivileged(port: &starmap::Port) -> Result<(), String> {
    if port.get() > 1024 {
        Ok(())
    } else {
        Err(format!(
            "the port must be above 1024, and {} is not",
            port.get()
        ))
    }
}

/// Refuses a URL whose scheme is not https.
fn https(url: &url::Url) -> Result<(), String> {
    if url.scheme() == "https" {
        Ok(())
    } else {
        Err("the catalogue is served over https".to_owned())
    }
}

/// Refuses a session key shorter than 16 characters.
#[expect(
    clippy::disallowed_methods,
    reason = "the check reads the secret it is given"
)]
fn long_key(key: &starmap::Secret) -> Result<(), &'static str> {
    if key.expose().len() >= 16 {
        Ok(())
    } else {
        Err("the session key is at least 16 characters")
    }
}

const LONG_KEY: (&str, &str) = ("SHELF_SESSION_KEY", "a-session-key-of-32-characters!!");

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test checks the secret that the struct reads"
)]
fn a_field_whose_check_passes_lets_the_struct_load() {
    let env = CheckedShelf::from_vars(vars(&[
        ("SHELF_PORT", "8080"),
        ("SHELF_CATALOGUE_URL", "https://shelf.example.org/"),
        LONG_KEY,
    ]))
    .expect("every check passes");

    assert_eq!(env.port.get(), 8080);
    assert!(env.catalogue.is_some());
    assert_eq!(env.session_key.expose(), LONG_KEY.1);
}

#[test]
fn a_failing_check_refuses_the_load_with_the_variable_its_reason_and_the_quoted_value() {
    let error = CheckedShelf::from_vars(vars(&[("SHELF_PORT", "80"), LONG_KEY]))
        .expect_err("80 is a privileged port");

    let ReadError::Refused(refusals) = &error else {
        panic!("a check refuses the load, and gave {error:?}");
    };
    let finding = refusals.iter().next().expect("one finding");
    assert_eq!(finding.name.as_str(), "SHELF_PORT");
    assert_eq!(
        finding.problem,
        starmap::Problem::Rejected {
            reason: "the port must be above 1024, and 80 is not".to_owned()
        }
    );
    assert_eq!(
        error.to_string(),
        "SHELF_PORT: the port must be above 1024, and 80 is not (the value is \"80\")"
    );
}

#[test]
fn a_failing_check_and_a_missing_variable_are_reported_in_one_refusal_in_the_spec_order() {
    let error = CheckedShelf::from_vars(vars(&[("SHELF_PORT", "80")])).expect_err("two findings");

    assert_eq!(
        error.to_string(),
        "SHELF_PORT: the port must be above 1024, and 80 is not (the value is \"80\"); \
         SHELF_SESSION_KEY is required in local mode and has no value"
    );
}

#[test]
fn a_value_that_its_type_refuses_is_not_passed_to_the_check() {
    let error = CheckedShelf::from_vars(vars(&[("SHELF_PORT", "80a"), LONG_KEY]))
        .expect_err("80a is not a port");

    assert_eq!(
        error.to_string(),
        "SHELF_PORT: the value is not a port from 1 to 65535 (the value is \"80a\")"
    );
}

#[test]
fn an_optional_field_whose_variable_is_unset_is_not_checked_and_a_set_one_is() {
    let unset = CheckedShelf::from_vars(vars(&[("SHELF_PORT", "8080"), LONG_KEY]));
    let plain = CheckedShelf::from_vars(vars(&[
        ("SHELF_PORT", "8080"),
        ("SHELF_CATALOGUE_URL", "http://shelf.example.org/"),
        LONG_KEY,
    ]))
    .expect_err("the catalogue is not https");

    assert!(
        unset
            .expect("an unset url is not checked")
            .catalogue
            .is_none()
    );
    assert_eq!(
        plain.to_string(),
        "SHELF_CATALOGUE_URL: the catalogue is served over https \
         (the value is \"http://shelf.example.org/\")"
    );
}

#[test]
fn a_withheld_field_check_receives_the_secret_and_its_refusal_holds_only_the_reason() {
    let error = CheckedShelf::from_vars(vars(&[
        ("SHELF_PORT", "8080"),
        ("SHELF_SESSION_KEY", "short-key"),
    ]))
    .expect_err("the session key is short");

    assert_eq!(
        error.to_string(),
        "SHELF_SESSION_KEY: the session key is at least 16 characters"
    );
    assert!(!format!("{error:?}").contains("short-key"));
}

/// A struct whose check refuses the default address of `listen.toml`.
#[derive(Debug, starmap::Env)]
#[starmap(spec = "tests/example/listen.toml")]
struct LoopbackListen {
    #[starmap("SHELF_LISTEN_ADDRESS", check = loopback)]
    #[expect(dead_code, reason = "the test reads the struct only to see it refused")]
    address: SocketAddr,
}

/// Refuses an address that is not a loopback address.
fn loopback(address: &SocketAddr) -> Result<(), String> {
    if address.ip().is_loopback() {
        Ok(())
    } else {
        Err(format!(
            "the api listens only on loopback, and {} is not",
            address.ip()
        ))
    }
}

#[test]
fn a_default_that_fails_the_check_refuses_the_load() {
    let error = LoopbackListen::from_vars(vars(&[("SHELF_STAGE", "dev")]))
        .expect_err("the default address 0.0.0.0:8080 is not loopback");

    assert_eq!(
        error.to_string(),
        "SHELF_LISTEN_ADDRESS: the api listens only on loopback, and 0.0.0.0 is not"
    );
}

/// A struct over a spec of families.
#[derive(Debug, starmap::Env)]
#[starmap(spec = "tests/example/regions.toml")]
struct Regions {
    #[starmap("SHELF_STAGE")]
    _stage: Stage,
    #[starmap(family = "SHELF_DB_URL")]
    databases: starmap::Family<url::Url>,
    #[starmap(family = "SHELF_TOKEN")]
    tokens: starmap::Family<Secret>,
}

#[test]
fn the_derive_reads_members_into_a_family_and_reports_them_in_key_order() {
    let regions = Regions::from_vars(vars(&[
        ("SHELF_DB_URL_US", "https://us.example/"),
        ("SHELF_DB_URL_EU", "https://eu.example/"),
        ("SHELF_TOKEN_ACME", "abc"),
    ]))
    .expect("the members fit the spec");

    assert_eq!(regions.databases.len(), 2);
    assert_eq!(regions.databases.keys().collect::<Vec<_>>(), ["EU", "US"]);
    assert_eq!(
        regions.databases.get("US").map(url::Url::as_str),
        Some("https://us.example/")
    );
    assert_eq!(
        regions
            .databases
            .iter()
            .map(|(key, url)| (key, url.as_str()))
            .collect::<Vec<_>>(),
        [("EU", "https://eu.example/"), ("US", "https://us.example/")]
    );
    assert_eq!(regions.tokens.keys().collect::<Vec<_>>(), ["ACME"]);
    assert!(regions.tokens.get("GLOBEX").is_none());
}

#[test]
fn a_family_field_is_described_by_its_prefix_and_the_type_of_its_values() {
    let field = Regions::FIELDS[1];

    assert_eq!(field.name, "SHELF_DB_URL");
    assert!(field.family);
    assert_eq!(field.rust_type, "url::Url");
    assert_eq!(
        (field.shape, field.presence, field.show),
        (Shape::Url, Presence::Always, Show::Shown)
    );
    assert!(!Regions::FIELDS[0].family);
}

#[test]
fn the_derive_refuses_an_unlisted_key_with_the_text_of_the_check() {
    let error = Regions::from_vars(vars(&[("SHELF_TOKEN_INITECH", "abc")]))
        .expect_err("INITECH is not a listed key");

    assert_eq!(
        error.to_string(),
        "SHELF_TOKEN_INITECH is set, and the family SHELF_TOKEN lists only the keys ACME and GLOBEX"
    );
}

#[test]
fn a_required_listed_family_that_passes_holds_every_listed_key() {
    let regions = Regions::from_vars(vars(&[
        ("SHELF_STAGE", "prod"),
        ("SHELF_TOKEN_ACME", "abc"),
        ("SHELF_TOKEN_GLOBEX", "def"),
    ]))
    .expect("every listed key is set");
    let missing = Regions::from_vars(vars(&[
        ("SHELF_STAGE", "prod"),
        ("SHELF_TOKEN_ACME", "abc"),
    ]))
    .expect_err("GLOBEX is required in production");

    assert_eq!(
        regions.tokens.keys().collect::<Vec<_>>(),
        ["ACME", "GLOBEX"]
    );
    assert_eq!(
        missing.to_string(),
        "SHELF_TOKEN_GLOBEX is required in production mode and has no value"
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

/// A struct over the example spec that reads only the port, as the
/// struct of a command that needs nothing else would.
#[derive(Debug, starmap::Env)]
#[starmap(spec = "tests/example/starmap.toml")]
struct PortOnly {
    #[starmap("SHELF_PORT")]
    port: starmap::Port,
}

#[test]
fn a_required_variable_the_struct_does_not_read_does_not_refuse_the_read() {
    let env = PortOnly::from_vars(vars(&[("SHELF_PORT", "8080")]))
        .expect("SHELF_SESSION_KEY is required, and the struct does not read it");

    assert_eq!(env.port, Port::try_from(8080).expect("8080 is a port"));
}

#[test]
fn an_invalid_value_of_a_variable_the_struct_does_not_read_does_not_refuse_the_read() {
    let env = PortOnly::from_vars(vars(&[
        ("SHELF_PORT", "8080"),
        ("SHELF_PAGE_SIZE", "500"),
        ("SHELF_DEBUG", "perhaps"),
    ]));

    assert!(env.is_ok(), "{env:?}");
}

#[test]
fn a_struct_read_reports_only_the_variables_its_fields_read() {
    let error =
        PortOnly::from_vars(vars(&[("SHELF_PAGE_SIZE", "500")])).expect_err("the port is missing");

    assert_eq!(
        error.to_string(),
        "SHELF_PORT is required in local mode and has no value"
    );
}

#[test]
fn a_struct_read_enforces_the_mode_variable_although_no_field_reads_it() {
    let invalid = PortOnly::from_vars(vars(&[("SHELF_ENV", "staging"), ("SHELF_PORT", "8080")]))
        .expect_err("staging is not a stage");
    let production = PortOnly::from_vars(vars(&[("SHELF_ENV", "prod"), ("SHELF_PORT", "8080")]))
        .expect("the mirror token is required in production, and the struct does not read it");

    assert_eq!(
        invalid.to_string(),
        "SHELF_ENV: the value is not one of dev, prod (the value is \"staging\")"
    );
    assert_eq!(
        production.port,
        Port::try_from(8080).expect("8080 is a port")
    );
}

#[test]
fn the_whole_file_check_still_refuses_a_variable_that_one_struct_does_not_read() {
    let spec = starmap::Spec::parse(PortOnly::SPEC).expect("the example spec parses");
    let bindings = starmap::capture(&spec, vars(&[("SHELF_PORT", "8080")]));
    let starmap::Verdict::Refused(refusals) =
        starmap::check(&spec, &bindings, starmap::Mode::Local)
    else {
        panic!("the session key is required");
    };

    assert_eq!(
        refusals.to_string(),
        "SHELF_SESSION_KEY is required in local mode and has no value"
    );
}

/// A struct over the spec of families that reads only the stage.
#[derive(Debug, starmap::Env)]
#[starmap(spec = "tests/example/regions.toml")]
struct StageOnly {
    #[starmap("SHELF_STAGE")]
    stage: Stage,
}

/// A struct over the spec of families that reads only the databases.
#[derive(Debug, starmap::Env)]
#[starmap(spec = "tests/example/regions.toml")]
struct DatabasesOnly {
    #[starmap(family = "SHELF_DB_URL")]
    databases: starmap::Family<url::Url>,
}

#[test]
fn a_member_of_a_family_the_struct_does_not_read_does_not_refuse_the_read() {
    let unlisted = DatabasesOnly::from_vars(vars(&[
        ("SHELF_DB_URL_US", "https://us.example/"),
        ("SHELF_TOKEN_INITECH", "abc"),
    ]))
    .expect("SHELF_TOKEN is a family the struct does not read");
    let production = StageOnly::from_vars(vars(&[("SHELF_STAGE", "prod")]))
        .expect("production requires each token, and the struct does not read them");

    assert_eq!(unlisted.databases.keys().collect::<Vec<_>>(), ["US"]);
    assert_eq!(production.stage, Stage::Prod);
}

#[test]
fn a_member_of_a_family_the_struct_reads_still_refuses_the_read() {
    let error = DatabasesOnly::from_vars(vars(&[("SHELF_DB_URL_US", "not a url")]))
        .expect_err("the member is not a url");

    assert!(
        error.to_string().starts_with("SHELF_DB_URL_US: "),
        "{error}"
    );
}

/// A struct over the spec with a group that reads one member of the group.
#[derive(Debug, starmap::Env)]
#[starmap(spec = "tests/example/mirror.toml")]
struct MirrorAccount {
    #[starmap("SHELF_MIRROR_ACCOUNT")]
    account: Option<String>,
}

#[test]
fn a_group_finding_refuses_the_read_only_on_a_member_the_struct_reads() {
    let unread_member_unset = MirrorAccount::from_vars(vars(&[("SHELF_MIRROR_ACCOUNT", "shelf")]))
        .expect("SHELF_MIRROR_KEY is unset, and the struct does not read it");
    let read_member_unset = MirrorAccount::from_vars(vars(&[("SHELF_MIRROR_KEY", "key")]))
        .expect_err("the struct reads the unset account, and the group needs it");

    assert_eq!(unread_member_unset.account.as_deref(), Some("shelf"));
    assert_eq!(
        read_member_unset.to_string(),
        "SHELF_MIRROR_ACCOUNT is unset, and the group mirror needs it because SHELF_MIRROR_KEY is set"
    );
}
