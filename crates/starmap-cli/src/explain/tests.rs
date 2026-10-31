use std::ffi::OsString;
use std::path::Path;

use starmap::{Name, Spec};

use crumbkit::{Call, Recovery};

use super::{list_again, row, value_text};
use crate::spec;

/// Returns the build plan's illustrative spec.
fn example() -> Spec {
    spec::load(Path::new("../starmap/tests/example/starmap.toml")).unwrap()
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
fn a_withheld_variable_with_a_default_hides_the_default_in_every_column() {
    let spec = Spec::parse(
        r#"
[variables.SHELF_SALT]
purpose = "Salts the catalogue's cache keys."
type = "string"
unset = { default = "pepper" }
show = "withheld"
source = "setting"
"#,
    )
    .unwrap();
    let bindings = starmap::capture(&spec, Vec::new());
    let row = row(
        &spec,
        "starmap.toml",
        &bindings,
        "SHELF_SALT".parse().unwrap(),
    )
    .unwrap();
    assert_eq!(row.unset, "default ••••••••");
    assert_eq!(row.value.to_string(), "•••••••• (default)");
    assert_eq!(
        serde_json::to_value(&row.value.0).unwrap(),
        serde_json::json!({ "state": "default" })
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
