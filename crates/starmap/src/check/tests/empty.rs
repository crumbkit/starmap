//! Tests for capturing and checking the empty value of a variable declared
//! with `empty = "value"`.

use std::ffi::{OsStr, OsString};

use super::{findings, name, ready, vars};
use crate::check::{Finding, Problem, Quote, capture, check, mode};
use crate::{Bindings, Bound, Mode, Secret, Spec, TypeError, Value, Verdict};

/// A spec whose variables each declare `empty = "value"`: a defaulted
/// integer, a required string, a withheld string, an absolute path and an
/// open family of strings.
const EMPTY: &str = r#"
[families.SHELF_LABEL]
purpose = "The label of one region, keyed by the region's code."
type = "string"
empty = "value"
unset = "optional"
show = "shown"
source = "setting"

[variables.SHELF_WORKERS]
purpose = "How many catalogue workers run."
type = { integer = { min = 1, max = 64 } }
empty = "value"
unset = { default = "5" }
show = "shown"
source = "setting"

[variables.SHELF_GREETING]
purpose = "The text the web view shows above the catalogue."
type = "string"
empty = "value"
unset = "required"
show = "shown"
source = "setting"

[variables.SHELF_MIRROR_TOKEN]
purpose = "The token shelf sends to the catalogue mirror."
type = "string"
empty = "value"
unset = "optional"
show = "withheld"
source = "secret"

[variables.XDG_STATE_HOME]
purpose = "The directory that holds shelf's state."
type = { path = { absolute = true } }
empty = "value"
unset = "optional"
show = "shown"
source = "inherited"
"#;

/// Returns the parsed [`EMPTY`].
fn spec() -> Spec {
    Spec::parse(EMPTY).unwrap()
}

/// Captures `pairs` with [`EMPTY`] and checks them in local mode.
fn verdict(pairs: &[(&str, &str)]) -> Verdict {
    let spec = spec();
    let bindings = capture(&spec, vars(pairs));
    check(&spec, &bindings, Mode::Local)
}

/// Returns the finding of the empty value of `text`, which is not a whole
/// number and is quoted as empty text.
fn not_an_integer(text: &str) -> Finding {
    Finding {
        value: Some(Quote::new(OsStr::new(""))),
        ..Finding::unquoted(name(text), Problem::Invalid(TypeError::NotInteger))
    }
}

#[test]
fn capture_binds_an_empty_value_of_a_variable_whose_empty_value_is_a_value() {
    let bindings = capture(
        &spec(),
        vars(&[
            ("SHELF_WORKERS", ""),
            ("SHELF_MIRROR_TOKEN", ""),
            ("SHELF_LABEL_EU", ""),
        ]),
    );

    assert!(matches!(
        bindings.get(&name("SHELF_WORKERS")),
        Some(Bound::Plain(value)) if value.is_empty()
    ));
    assert!(matches!(
        bindings.get(&name("SHELF_MIRROR_TOKEN")),
        Some(Bound::Secret(secret)) if secret.is_empty()
    ));
    assert!(matches!(
        bindings.get(&name("SHELF_LABEL_EU")),
        Some(Bound::Plain(value)) if value.is_empty()
    ));
}

#[test]
fn an_empty_later_value_replaces_an_earlier_one_when_an_empty_value_is_a_value() {
    let bindings = capture(
        &spec(),
        vars(&[("SHELF_GREETING", "Hello"), ("SHELF_GREETING", "")]),
    );

    assert!(matches!(
        bindings.get(&name("SHELF_GREETING")),
        Some(Bound::Plain(value)) if value.is_empty()
    ));
}

#[test]
fn an_empty_value_of_an_integer_is_invalid_and_its_default_applies_only_when_it_is_absent() {
    let empty = findings(verdict(&[("SHELF_WORKERS", ""), ("SHELF_GREETING", "Hi")]));
    let absent = ready(verdict(&[("SHELF_GREETING", "Hi")]));

    assert_eq!(empty, [not_an_integer("SHELF_WORKERS")]);
    assert_eq!(
        absent.value(&name("SHELF_WORKERS")),
        Some(&Value::Integer(5))
    );
}

#[test]
fn an_empty_value_satisfies_a_required_string() {
    let environment = ready(verdict(&[("SHELF_GREETING", "")]));

    assert_eq!(
        environment.value(&name("SHELF_GREETING")),
        Some(&Value::String(String::new()))
    );
}

#[test]
fn an_empty_value_of_a_withheld_string_is_held_as_an_empty_secret() {
    let environment = ready(verdict(&[
        ("SHELF_GREETING", "Hi"),
        ("SHELF_MIRROR_TOKEN", ""),
    ]));

    assert!(
        environment
            .secret(&name("SHELF_MIRROR_TOKEN"))
            .is_some_and(Secret::is_empty)
    );
}

#[test]
fn an_absolute_path_type_counts_an_empty_value_as_unset() {
    let environment = ready(verdict(&[("SHELF_GREETING", "Hi"), ("XDG_STATE_HOME", "")]));

    assert_eq!(environment.value(&name("XDG_STATE_HOME")), None);
}

#[test]
fn a_family_member_whose_empty_value_is_a_value_holds_empty_text() {
    let environment = ready(verdict(&[("SHELF_GREETING", "Hi"), ("SHELF_LABEL_EU", "")]));

    let prefix = name("SHELF_LABEL");
    let keys: Vec<&str> = environment.members(&prefix).map(|(key, _)| key).collect();
    assert_eq!(keys, ["EU"]);
    assert_eq!(
        environment.value(&name("SHELF_LABEL_EU")),
        Some(&Value::String(String::new()))
    );
}

#[test]
fn an_empty_bound_value_from_outside_capture_is_read_by_the_type() {
    let spec = spec();
    let mut bindings = Bindings::new();
    bindings.bind(name("SHELF_GREETING"), Bound::Plain(OsString::new()));
    bindings.bind(name("SHELF_WORKERS"), Bound::Plain(OsString::new()));

    assert_eq!(
        findings(check(&spec, &bindings, Mode::Local)),
        [not_an_integer("SHELF_WORKERS")]
    );
}

#[test]
fn an_empty_mode_value_is_read_by_the_mode_variables_type() {
    let spec = Spec::parse(
        r#"
[mode]
variable = "SHELF_ENV"
production = ["prod"]

[variables.SHELF_ENV]
purpose = "Selects local or production behaviour."
type = { enumeration = ["dev", "prod"] }
empty = "value"
unset = { default = "prod" }
show = "shown"
source = "setting"
"#,
    )
    .unwrap();
    let bindings = capture(&spec, vars(&[("SHELF_ENV", "")]));

    assert_eq!(mode(&spec, &bindings), Mode::Local);
    assert!(matches!(
        findings(check(&spec, &bindings, Mode::Local)).as_slice(),
        [found] if found.name == name("SHELF_ENV")
            && matches!(found.problem, Problem::Invalid(TypeError::EmptyChoice(_)))
    ));
}
