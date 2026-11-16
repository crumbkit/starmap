use std::ffi::OsString;

use serde_json::{Value, json};

use super::{EnvironmentFailure, listing};

/// The example spec of a tool named shelf, which declares `XDG_STATE_HOME`.
const SHELF: &str = include_str!("../../starmap/tests/example/starmap.toml");

/// The spec whose fields defer their rules.
const DEFERRED: &str = include_str!("../../starmap/tests/example/deferred.toml");

/// The spec whose variables count an empty value as a value.
const EMPTY: &str = include_str!("../../starmap/tests/example/empty.toml");

/// Returns the JSON rows that `self environment list` prints for `tool` with
/// the declaration `spec` and `vars` as the process environment.
fn rows(tool: &str, spec: &str, vars: &[(&str, &str)]) -> Vec<Value> {
    let vars: Vec<(OsString, OsString)> = vars
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect();
    let listing = listing(tool, spec, &vars).unwrap();
    match serde_json::to_value(&listing).unwrap() {
        Value::Array(rows) => rows,
        other => panic!("the listing serialises as an array, not {other}"),
    }
}

/// Returns the row of `name`.
fn find(rows: &[Value], name: &str) -> Value {
    rows.iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("no row for {name}"))
        .clone()
}

#[test]
fn each_variable_of_the_tool_has_a_row_before_crumbkits() {
    let rows = rows("shelf", SHELF, &[]);
    let names: Vec<&str> = rows
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();

    assert_eq!(names.first(), Some(&"SHELF_ENV"));
    assert!(names.contains(&"SHELF_MIRROR_TOKEN"));
    let first_crumbkit = rows
        .iter()
        .position(|row| row.get("owner").is_some())
        .unwrap();
    assert!(
        rows[first_crumbkit..]
            .iter()
            .all(|row| row["owner"] == "crumbkit")
    );
    assert!(
        rows[..first_crumbkit]
            .iter()
            .all(|row| row.get("owner").is_none())
    );
}

#[test]
fn a_variable_is_set_default_unset_or_invalid_at_the_top_level() {
    let rows = rows(
        "shelf",
        SHELF,
        &[("SHELF_PORT", "8080"), ("SHELF_DEBUG", "maybe")],
    );

    assert_eq!(find(&rows, "SHELF_PORT")["state"], "set");
    assert_eq!(find(&rows, "SHELF_PAGE_SIZE")["state"], "default");
    assert_eq!(find(&rows, "SHELF_CATALOGUE_URL")["state"], "unset");
    assert_eq!(find(&rows, "SHELF_DEBUG")["state"], "invalid");
}

#[test]
fn a_row_has_the_columns_of_explain() {
    let row = find(
        &rows("shelf", SHELF, &[("SHELF_PORT", "8080")]),
        "SHELF_PORT",
    );

    assert_eq!(row["type"], "port");
    assert_eq!(row["unset"], "required");
    assert_eq!(row["source"], "provided multiverse/port");
    assert_eq!(row["value"], json!({"state": "set", "text": "8080"}));
    assert_eq!(row["show"], "shown");
}

#[test]
fn a_withheld_value_has_no_text_in_any_state() {
    let token = "hunter2-must-never-print";
    let set = find(
        &rows("shelf", SHELF, &[("SHELF_MIRROR_TOKEN", token)]),
        "SHELF_MIRROR_TOKEN",
    );
    let unset = find(&rows("shelf", SHELF, &[]), "SHELF_MIRROR_TOKEN");

    assert_eq!(set["state"], "set");
    assert_eq!(set["value"], json!({"state": "set"}));
    assert!(!set.to_string().contains(token));
    assert_eq!(unset["state"], "unset");
}

#[test]
fn an_empty_value_is_set_for_a_variable_that_declares_empty_is_a_value() {
    let rows = rows("shelf", EMPTY, &[("SHELF_GREETING", "")]);
    let greeting = find(&rows, "SHELF_GREETING");

    assert_eq!(greeting["state"], "set");
    assert_eq!(greeting["value"]["text"], "");
}

#[test]
fn an_empty_value_is_unset_for_a_variable_that_does_not() {
    let rows = rows("shelf", SHELF, &[("SHELF_DEBUG", "")]);

    assert_eq!(find(&rows, "SHELF_DEBUG")["state"], "default");
}

#[test]
fn a_deferred_variable_that_is_unset_is_unset_and_not_invalid() {
    let rows = rows("shelf", DEFERRED, &[]);
    let states: Vec<(&str, &str)> = rows
        .iter()
        .filter(|row| row.get("owner").is_none())
        .map(|row| {
            (
                row["name"].as_str().unwrap(),
                row["state"].as_str().unwrap(),
            )
        })
        .collect();

    assert!(!states.is_empty());
    assert!(
        states.iter().all(|(_, state)| *state != "invalid"),
        "{states:?}"
    );
    assert!(
        states.contains(&("SHELF_CATALOGUE_URL", "unset")),
        "{states:?}"
    );
    assert!(states.contains(&("SHELF_TOKEN", "unset")), "{states:?}");
}

#[test]
fn crumbkits_variables_start_with_the_tools_name() {
    let rows = rows("my-tool", SHELF, &[("MY_TOOL_PROGRAM", "flightdeck")]);
    let program = find(&rows, "MY_TOOL_PROGRAM");

    assert_eq!(program["owner"], "crumbkit");
    assert_eq!(program["state"], "set");
    assert_eq!(program["value"]["text"], "flightdeck");
    assert_eq!(find(&rows, "MY_TOOL_OUTPUT_BUDGET")["state"], "unset");
}

#[test]
fn a_program_name_that_is_not_one_is_invalid() {
    let rows = rows("shelf", SHELF, &[("SHELF_PROGRAM", "Not A Name")]);

    assert_eq!(find(&rows, "SHELF_PROGRAM")["state"], "invalid");
}

#[test]
fn a_variable_that_the_tool_declares_keeps_the_tools_declaration() {
    let rows = rows("shelf", SHELF, &[("XDG_STATE_HOME", "/state")]);
    let declared: Vec<&Value> = rows
        .iter()
        .filter(|row| row["name"] == "XDG_STATE_HOME")
        .collect();

    assert_eq!(declared.len(), 1);
    assert!(declared[0].get("owner").is_none());
}

#[test]
fn colour_variables_are_set_by_any_non_empty_value() {
    let rows = rows("shelf", DEFERRED, &[("NO_COLOR", "1")]);

    assert_eq!(find(&rows, "NO_COLOR")["state"], "set");
    assert_eq!(find(&rows, "CLICOLOR")["state"], "unset");
}

#[test]
fn a_declaration_that_does_not_parse_is_a_failure() {
    let failure: EnvironmentFailure = listing("shelf", "[variables.X", &[]).unwrap_err();

    assert!(
        failure
            .to_string()
            .starts_with("the tool's embedded declaration does not parse"),
        "{failure}"
    );
}
