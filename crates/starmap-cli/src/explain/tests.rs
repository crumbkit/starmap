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
fn an_empty_value_shows_the_default_or_unset() {
    assert_eq!(
        columns("SHELF_DEBUG", &[("SHELF_DEBUG", "")])[4],
        "false (default)"
    );
    assert_eq!(columns("SHELF_PORT", &[("SHELF_PORT", "")])[4], "unset");
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

#[test]
fn a_withheld_value_that_does_not_match_its_pattern_is_marked_invalid_without_its_text() {
    let spec = Spec::parse(
        "[variables.SHELF_MIRROR_KEY]\npurpose = \"The mirror key.\"\n\
         type = { string = { pattern = \"sk-[a-z0-9]+\" } }\nunset = \"required\"\n\
         show = \"withheld\"\nsource = \"secret\"\n",
    )
    .unwrap();
    let name: Name = "SHELF_MIRROR_KEY".parse().unwrap();
    let value = |text: &str| {
        let vars = [(OsString::from("SHELF_MIRROR_KEY"), OsString::from(text))];
        let bindings = starmap::capture(&spec, vars);
        row(&spec, "starmap.toml", &bindings, name.clone())
            .unwrap()
            .value
            .to_string()
    };

    assert_eq!(value("sk-abc123"), "\u{2022}".repeat(8));
    let refused = value("pk-hidden-value");
    assert_eq!(refused, format!("{} (invalid)", "\u{2022}".repeat(8)));
    assert!(!refused.contains("pk-hidden-value"));
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

#[test]
fn explain_prints_the_same_row_for_a_variable_with_and_without_tool_tables() {
    let tools = format!(
        "[tool.codegen]\nmodule = \"settings\"\n\n{}tool.codegen = {{ field = \"listen\" }}\n",
        LISTEN.trim_start()
    );
    let json = |text: &str| {
        let spec = Spec::parse(text).unwrap();
        let bindings = starmap::capture(&spec, Vec::<(OsString, OsString)>::new());
        let row = row(
            &spec,
            "starmap.toml",
            &bindings,
            "SHELF_LISTEN".parse().unwrap(),
        )
        .unwrap();
        serde_json::to_value(row).unwrap()
    };

    assert_eq!(json(&tools), json(LISTEN));
}

/// Returns the JSON that `starmap explain --json` prints for one optional
/// setting whose extra fields are `extra`.
fn optional_bucket(extra: &str) -> serde_json::Value {
    let spec = Spec::parse(&format!(
        "[variables.SHELF_BUCKET]\npurpose = \"The bucket.\"\n{extra}type = \"string\"\n\
         unset = \"optional\"\nshow = \"shown\"\nsource = \"setting\"\n"
    ))
    .unwrap();
    let bindings = starmap::capture(&spec, Vec::<(OsString, OsString)>::new());
    let row = row(
        &spec,
        "starmap.toml",
        &bindings,
        "SHELF_BUCKET".parse().unwrap(),
    )
    .unwrap();
    serde_json::to_value(row).unwrap()
}

#[test]
fn a_variable_with_an_effect_shows_it_in_json() {
    let json = optional_bucket("effect = \"Calls are not recorded.\"\n");

    assert_eq!(json["effect"], serde_json::json!("Calls are not recorded."));
}

#[test]
fn a_variable_without_an_effect_has_no_effect_field_in_json() {
    let json = optional_bucket("");

    assert!(json.get("effect").is_none(), "{json}");
}

#[test]
fn the_effect_is_the_long_column_after_the_purpose() {
    use crumbkit::Row;

    let long = super::Explained::LONG_COLUMNS;
    let purpose = long.iter().position(|column| *column == "purpose");
    let effect = long.iter().position(|column| *column == "effect");

    assert_eq!(effect, purpose.map(|purpose| purpose + 1));
}

/// A database url setting that allows the schemes postgres and postgresql.
const DATABASE: &str = r#"
[variables.SHELF_DATABASE_URL]
purpose = "The database shelf keeps its catalogue in."
type = { url = { schemes = ["postgres", "postgresql"] } }
unset = "required"
show = "shown"
source = "setting"
"#;

/// Returns the row of `SHELF_DATABASE_URL` in [`DATABASE`] when the shell
/// holds `value`, as `starmap explain --json` prints it.
fn database_row(value: &str) -> serde_json::Value {
    let spec = Spec::parse(DATABASE).unwrap();
    let vars = [(OsString::from("SHELF_DATABASE_URL"), OsString::from(value))];
    let bindings = starmap::capture(&spec, vars);
    let name: Name = "SHELF_DATABASE_URL".parse().unwrap();
    serde_json::to_value(row(&spec, "starmap.toml", &bindings, name).unwrap()).unwrap()
}

#[test]
fn a_url_with_schemes_shows_them_in_its_type() {
    let row = database_row("postgres://localhost/shelf");

    assert_eq!(row["type"], "url (postgres, postgresql)");
    assert_eq!(row["value"]["state"], "set");
}

#[test]
fn a_url_whose_scheme_is_not_listed_is_marked_invalid() {
    let row = database_row("mysql://localhost/shelf");

    assert_eq!(row["value"]["state"], "invalid");
}

/// A spec with a named enumeration `log-level`, which `SHELF_LOG_LEVEL`
/// uses, and an inline one, which `SHELF_STAGE` uses.
const LOG_LEVEL: &str = r#"
[enumerations.log-level]
debug = "Logs every request and its timing."
info = "Logs each request once."
warn = "Logs only failures and slow requests."

[variables.SHELF_LOG_LEVEL]
purpose = "How much the api process logs."
type = { enumeration = "log-level" }
unset = { default = "info" }
show = "shown"
source = "setting"

[variables.SHELF_STAGE]
purpose = "The stage."
type = { enumeration = ["dev", "prod"] }
unset = "required"
show = "shown"
source = "setting"
"#;

/// Returns the row of `name` in [`LOG_LEVEL`] with no value in the shell.
fn log_level_row(name: &str) -> super::Explained {
    let spec = Spec::parse(LOG_LEVEL).unwrap();
    let bindings = starmap::capture(&spec, Vec::<(OsString, OsString)>::new());
    row(&spec, "starmap.toml", &bindings, name.parse().unwrap()).unwrap()
}

#[test]
fn a_named_enumeration_shows_its_name_before_its_values_in_the_type() {
    assert_eq!(
        log_level_row("SHELF_LOG_LEVEL").kind,
        "log-level: one of debug, info, warn"
    );
}

#[test]
fn the_values_column_lists_each_value_with_its_description() {
    let values = log_level_row("SHELF_LOG_LEVEL").values.unwrap();

    assert_eq!(
        values.to_string(),
        "debug: Logs every request and its timing.; info: Logs each request once.; warn: Logs \
         only failures and slow requests."
    );
}

#[test]
fn json_holds_the_enumeration_name_and_each_value_with_its_description() {
    let json = serde_json::to_value(log_level_row("SHELF_LOG_LEVEL")).unwrap();

    assert_eq!(json["enumeration"], "log-level");
    assert_eq!(
        json["values"],
        serde_json::json!([
            { "value": "debug", "description": "Logs every request and its timing." },
            { "value": "info", "description": "Logs each request once." },
            { "value": "warn", "description": "Logs only failures and slow requests." },
        ])
    );
}

#[test]
fn a_variable_without_a_named_enumeration_has_no_enumeration_or_values_in_json() {
    let json = serde_json::to_value(log_level_row("SHELF_STAGE")).unwrap();

    assert_eq!(json["type"], "one of dev, prod");
    assert!(json.get("enumeration").is_none(), "{json}");
    assert!(json.get("values").is_none(), "{json}");
}

#[test]
fn the_values_are_the_last_long_column() {
    use crumbkit::Row;

    assert_eq!(super::Explained::LONG_COLUMNS.last(), Some(&"values"));
}
