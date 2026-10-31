//! Tests for the checked environment and how it applies to a command.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::process::Command;

use crate::{Environment, Mode, Name, Spec, Verdict, capture, check};

/// A spec with a shown port, a defaulted boolean, an optional path and a
/// withheld token.
const SPEC: &str = r#"
[variables.SHELF_PORT]
purpose = "The port the api process listens on."
type = "port"
unset = { default = "8080" }
show = "shown"
source = "setting"

[variables.SHELF_DEBUG]
purpose = "Logs each catalogue query."
type = "boolean"
unset = { default = "FALSE" }
show = "shown"
source = "setting"

[variables.XDG_STATE_HOME]
purpose = "The directory that holds shelf's state."
type = "path"
unset = "optional"
show = "shown"
source = "inherited"

[variables.SHELF_MIRROR_TOKEN]
purpose = "The token shelf sends to the catalogue mirror."
type = "string"
unset = "optional"
show = "withheld"
source = "secret"
"#;

/// Returns the name `text`.
fn name(text: &str) -> Name {
    text.parse().unwrap()
}

/// Checks `pairs` against [`SPEC`] in local mode.
fn environment(pairs: &[(&str, &str)]) -> Environment {
    let spec = Spec::parse(SPEC).unwrap();
    let vars = pairs
        .iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)));
    match check(&spec, &capture(&spec, vars), Mode::Local) {
        Verdict::Ready(environment) => environment,
        Verdict::Refused(refusals) => panic!("the check refused: {refusals:?}"),
    }
}

/// Returns a command that the tests configure and never start.
#[expect(
    clippy::disallowed_methods,
    reason = "the tests read the command's environment and never start it"
)]
fn command() -> Command {
    Command::new("child")
}

/// Returns each variable that `command` sets or removes, with `None` for a
/// removed one.
fn changes(command: &Command) -> BTreeMap<String, Option<String>> {
    command
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect()
}

#[test]
fn apply_sets_each_value_and_removes_each_unset_declared_name() {
    let mut command = command();
    environment(&[("SHELF_PORT", "9000"), ("SHELF_MIRROR_TOKEN", "hunter2")]).apply(&mut command);

    let expected = BTreeMap::from([
        ("SHELF_DEBUG".to_owned(), Some("false".to_owned())),
        ("SHELF_MIRROR_TOKEN".to_owned(), Some("hunter2".to_owned())),
        ("SHELF_PORT".to_owned(), Some("9000".to_owned())),
        ("XDG_STATE_HOME".to_owned(), None),
    ]);
    assert_eq!(changes(&command), expected);
}

#[test]
fn apply_leaves_every_undeclared_variable_of_the_command_as_it_was() {
    let mut command = command();
    command.env("LANG", "C").env_remove("TERM");
    environment(&[("SHELF_THEME", "dark")]).apply(&mut command);

    let changes = changes(&command);
    assert_eq!(changes.get("LANG"), Some(&Some("C".to_owned())));
    assert_eq!(changes.get("TERM"), Some(&None));
    assert!(!changes.contains_key("SHELF_THEME"));
    assert_eq!(changes.len(), 6);
}

#[test]
fn apply_writes_a_value_as_text_its_form_reads_back() {
    let mut command = command();
    environment(&[("SHELF_DEBUG", "Yes")]).apply(&mut command);

    let debug = command
        .get_envs()
        .find(|(key, _)| *key == OsStr::new("SHELF_DEBUG"))
        .and_then(|(_, value)| value);
    assert_eq!(debug, Some(OsStr::new("true")));
}

#[test]
fn a_withheld_value_is_reachable_only_through_secret() {
    let environment = environment(&[("SHELF_MIRROR_TOKEN", "hunter2")]);
    let token = name("SHELF_MIRROR_TOKEN");

    assert_eq!(environment.value(&token), None);
    assert!(environment.secret(&token).is_some());
    assert!(!format!("{environment:?}").contains("hunter2"));
}

#[test]
fn secret_returns_nothing_for_a_shown_variable() {
    let environment = environment(&[]);

    assert!(environment.secret(&name("SHELF_PORT")).is_none());
    assert!(environment.value(&name("SHELF_PORT")).is_some());
}

#[test]
fn an_environment_reports_the_mode_it_was_checked_in() {
    assert_eq!(environment(&[]).mode(), Mode::Local);
}
