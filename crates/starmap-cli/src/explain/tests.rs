use std::ffi::OsString;
use std::path::Path;

use starmap::{Name, Spec};

use crumbkit::{Call, Recovery};

use super::{list_again, row, value_text};
use crate::spec;

/// Returns the example spec in `crates/starmap/tests/example/starmap.toml`.
fn example() -> Spec {
    spec::open(Path::new("../starmap/tests/example/starmap.toml"))
        .unwrap()
        .spec
}

/// Returns the text of each column of the row of `name`, with `vars` as
/// the shell's values.
fn columns(name: &str, vars: &[(&str, &str)]) -> [String; 5] {
    let spec = example();
    let vars = vars
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)));
    let bindings = starmap::capture(&spec, vars);
    let name: Name = name.parse().unwrap();
    let row = row(&spec, "starmap.toml", &bindings, name).unwrap();
    [
        row.name,
        row.kind,
        row.unset,
        row.source,
        row.value.to_string(),
    ]
}

#[test]
fn an_enumeration_with_a_default_shows_its_values_and_the_default_value() {
    assert_eq!(
        columns("SHELF_ENV", &[]),
        [
            "SHELF_ENV",
            "one of dev, prod",
            "default dev",
            "setting",
            "dev (default)"
        ]
    );
}

#[test]
fn a_provided_port_shows_its_provider_kind_and_the_shell_value() {
    assert_eq!(
        columns("SHELF_PORT", &[("SHELF_PORT", "8080")]),
        [
            "SHELF_PORT",
            "port",
            "required",
            "provided multiverse/port",
            "8080"
        ]
    );
}

#[test]
fn a_secret_that_differs_between_modes_names_both_rules_and_withholds_its_value() {
    assert_eq!(
        columns("SHELF_MIRROR_TOKEN", &[("SHELF_MIRROR_TOKEN", "hunter2")]),
        [
            "SHELF_MIRROR_TOKEN",
            "string",
            "optional; production required",
            "secret, 1Password",
            "••••••••",
        ]
    );
}

#[test]
fn an_integer_shows_its_range_and_a_path_bind_is_named() {
    assert_eq!(columns("SHELF_PAGE_SIZE", &[])[1], "integer 1..=100");
    assert_eq!(columns("SHELF_DATABASE", &[])[3], "setting, path bind");
}

#[test]
fn a_relative_path_where_an_absolute_one_is_needed_counts_as_unset() {
    assert_eq!(
        columns("XDG_STATE_HOME", &[("XDG_STATE_HOME", "relative/state")])[4],
        "unset"
    );
    assert_eq!(
        columns(
            "XDG_STATE_HOME",
            &[("XDG_STATE_HOME", "/home/a/.local/state")]
        )[4],
        "/home/a/.local/state"
    );
}

#[test]
fn a_value_that_does_not_fit_its_type_is_marked_invalid() {
    assert_eq!(
        columns("SHELF_DEBUG", &[("SHELF_DEBUG", "maybe")])[4],
        "maybe (invalid)"
    );
}

#[test]
fn a_name_the_spec_does_not_declare_is_a_failure_of_that_name() {
    let spec = example();
    let bindings = starmap::capture(&spec, Vec::new());
    let failure = row(
        &spec,
        "starmap.toml",
        &bindings,
        "SHELF_THEME".parse().unwrap(),
    )
    .unwrap_err();
    assert_eq!(
        failure.to_string(),
        "starmap.toml does not declare SHELF_THEME"
    );
}

#[test]
fn a_value_reads_as_the_text_an_environment_holds() {
    let spec = example();
    let name: Name = "SHELF_PAGE_SIZE".parse().unwrap();
    let starmap::Unset::Default(value) = spec
        .variable(&name)
        .unwrap()
        .unset()
        .in_mode(starmap::Mode::Local)
    else {
        panic!("SHELF_PAGE_SIZE has a default");
    };
    assert_eq!(value_text(value), "20");
}

#[test]
fn the_recovery_of_an_unknown_name_keeps_a_spec_file_that_is_not_the_default() {
    assert_eq!(
        list_again("other.toml"),
        Recovery::Run(Call::new(&["explain"]).value("spec", "other.toml"))
    );
    assert_eq!(
        list_again("starmap.toml"),
        Recovery::Run(Call::new(&["explain"]))
    );
}

/// A socket address setting with a default.
const LISTEN: &str = r#"
[variables.SHELF_LISTEN]
purpose = "The address the api process listens on."
type = "socket-address"
unset = { default = "0.0.0.0:8080" }
show = "shown"
source = "setting"
"#;

#[test]
fn a_socket_address_is_named_as_its_type_and_shows_its_value() {
    let spec = Spec::parse(LISTEN).unwrap();
    let name: Name = "SHELF_LISTEN".parse().unwrap();
    let vars = [(OsString::from("SHELF_LISTEN"), OsString::from("[::1]:9090"))];
    let bindings = starmap::capture(&spec, vars);
    let shell = row(&spec, "starmap.toml", &bindings, name.clone()).unwrap();
    assert_eq!(shell.kind, "ip socket address");
    assert_eq!(shell.value.to_string(), "[::1]:9090");

    let bindings = starmap::capture(&spec, Vec::new());
    let default = row(&spec, "starmap.toml", &bindings, name).unwrap();
    assert_eq!(default.value.to_string(), "0.0.0.0:8080 (default)");
}

#[test]
fn a_string_with_a_pattern_shows_the_pattern_in_its_type() {
    let spec = Spec::parse(
        "[variables.SHELF_REVISION]\npurpose = \"The revision.\"\n\
         type = { string = { pattern = \"[0-9a-f]{40}\" } }\nunset = \"optional\"\n\
         show = \"shown\"\nsource = \"setting\"\n",
    )
    .unwrap();
    let name: Name = "SHELF_REVISION".parse().unwrap();
    let bindings = starmap::capture(&spec, Vec::<(OsString, OsString)>::new());

    let row = row(&spec, "starmap.toml", &bindings, name).unwrap();

    assert_eq!(row.kind, "string matching [0-9a-f]{40}");
}

/// Returns the `bind` and `cache` fields that `starmap explain --json`
/// prints for the one secret whose bind is `bind`.
fn secret_bind(bind: &str) -> (serde_json::Value, serde_json::Value) {
    let spec = Spec::parse(&format!(
        "[variables.SHELF_TOKEN]\npurpose = \"The token.\"\ntype = \"string\"\n\
         unset = \"required\"\nshow = \"withheld\"\nsource = \"secret\"\nbind = {bind}\n"
    ))
    .unwrap();
    let bindings = starmap::capture(&spec, Vec::<(OsString, OsString)>::new());
    let row = row(
        &spec,
        "starmap.toml",
        &bindings,
        "SHELF_TOKEN".parse().unwrap(),
    )
    .unwrap();
    let json = serde_json::to_value(row).unwrap();
    (json["bind"].clone(), json["cache"].clone())
}

#[test]
fn a_bind_that_no_cache_may_keep_reports_cache_never_beside_its_plain_reference() {
    assert_eq!(
        secret_bind(r#"{ one-password = "op://Personal/shelf/token", cache = false }"#),
        (
            serde_json::json!("op://Personal/shelf/token"),
            serde_json::json!("never")
        )
    );
}

#[test]
fn a_bind_that_a_cache_may_keep_shows_its_reference_and_no_cache_field() {
    for bind in [
        r#"{ one-password = "op://Personal/shelf/token" }"#,
        r#"{ one-password = "op://Personal/shelf/token", cache = true }"#,
    ] {
        assert_eq!(
            secret_bind(bind),
            (
                serde_json::json!("op://Personal/shelf/token"),
                serde_json::Value::Null
            ),
            "{bind}"
        );
    }
}

#[test]
fn a_field_named_like_the_cache_policy_is_shown_as_it_is_written() {
    assert_eq!(
        secret_bind(r#"{ one-password = "op://Personal/shelf/token, never cached" }"#),
        (
            serde_json::json!("op://Personal/shelf/token, never cached"),
            serde_json::Value::Null
        )
    );
}
