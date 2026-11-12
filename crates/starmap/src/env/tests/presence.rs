//! Tests for a variable that reads its presence: how the check holds it
//! and how it reaches a field and a child process.

use std::ffi::{OsStr, OsString};

use super::boolean::{checked, ready};
use super::{changes, command, name};
use crate::field::FieldType;
use crate::{Environment, Mode, Spec, Value, Verdict, capture, check, mode};

/// A spec with `NO_COLOR`, which reads its presence, and an ordinary
/// optional boolean.
const SPEC: &str = r#"
[variables.NO_COLOR]
purpose = "Turns colour off when it is set."
type = "boolean"
presence = true
unset = "optional"
show = "shown"
source = "inherited"

[variables.SHELF_DEBUG]
purpose = "Logs each catalogue query."
type = "boolean"
unset = "optional"
show = "shown"
source = "setting"
"#;

/// Returns what `environment` does to `NO_COLOR` in a child process: the
/// value it sets, or `None` when it removes the variable.
fn applied(environment: &Environment) -> Option<String> {
    let mut command = command();
    environment.apply(&mut command);
    changes(&command)
        .remove("NO_COLOR")
        .expect("apply sets or removes each declared variable")
}

#[test]
fn any_value_of_a_presence_variable_reads_as_true_and_reaches_the_child_unchanged() {
    let no_color = name("NO_COLOR");
    for set in ["", "1", "0", "false", "anything at all"] {
        let environment = ready(checked(SPEC, &[("NO_COLOR", set)]));

        assert_eq!(bool::read(&environment, &no_color), Ok(true), "{set:?}");
        assert_eq!(
            environment.value(&no_color),
            Some(&Value::Boolean(true)),
            "{set:?}"
        );
        assert_eq!(applied(&environment).as_deref(), Some(set));
    }
}

#[test]
fn an_absent_presence_variable_reads_as_false_and_stays_unset_in_the_child() {
    let environment = ready(checked(SPEC, &[]));
    let no_color = name("NO_COLOR");

    assert_eq!(bool::read(&environment, &no_color), Ok(false));
    assert_eq!(environment.value(&no_color), Some(&Value::Boolean(false)));
    assert_eq!(applied(&environment), None);
}

#[test]
fn an_empty_value_still_counts_as_unset_for_a_variable_without_presence() {
    let environment = ready(checked(SPEC, &[("SHELF_DEBUG", ""), ("NO_COLOR", "")]));

    assert_eq!(environment.value(&name("SHELF_DEBUG")), None);
    assert_eq!(applied(&environment).as_deref(), Some(""));
}

#[test]
fn the_last_value_of_a_presence_variable_is_bound_even_when_it_is_empty() {
    let environment = ready(checked(SPEC, &[("NO_COLOR", "1"), ("NO_COLOR", "")]));

    assert_eq!(applied(&environment).as_deref(), Some(""));
}

#[cfg(unix)]
#[test]
fn a_presence_value_that_is_not_unicode_reads_as_true_and_reaches_the_child_unchanged() {
    use std::os::unix::ffi::OsStringExt as _;

    let spec = Spec::parse(SPEC).unwrap();
    let raw = OsString::from_vec(vec![0xff, 0xfe]);
    let vars = [(OsString::from("NO_COLOR"), raw.clone())];
    let environment = ready(check(&spec, &capture(&spec, vars), Mode::Local));
    let mut command = command();

    environment.apply(&mut command);

    assert_eq!(bool::read(&environment, &name("NO_COLOR")), Ok(true));
    let set = command
        .get_envs()
        .find(|(key, _)| *key == OsStr::new("NO_COLOR"))
        .and_then(|(_, value)| value);
    assert_eq!(set, Some(raw.as_os_str()));
}

/// A spec whose required variable reads its presence.
const REQUIRED: &str = r#"
[variables.SHELF_ACCEPT_TERMS]
purpose = "Records that the operator accepts the catalogue's terms."
type = "boolean"
presence = true
unset = "required"
show = "shown"
source = "setting"
"#;

#[test]
fn a_required_presence_variable_is_missing_when_absent_and_passes_with_an_empty_value() {
    let Verdict::Refused(refusals) = checked(REQUIRED, &[]) else {
        panic!("the absent variable passed the check");
    };

    assert_eq!(
        refusals.to_string(),
        "SHELF_ACCEPT_TERMS is required and has no value"
    );
    let environment = ready(checked(REQUIRED, &[("SHELF_ACCEPT_TERMS", "")]));
    assert_eq!(
        bool::read(&environment, &name("SHELF_ACCEPT_TERMS")),
        Ok(true)
    );
}

/// A spec with a group of a presence variable and an optional setting.
const GROUP: &str = r#"
[groups.plain]
variables = ["NO_COLOR", "SHELF_WIDTH"]

[variables.NO_COLOR]
purpose = "Turns colour off when it is set."
type = "boolean"
presence = true
unset = "optional"
show = "shown"
source = "inherited"

[variables.SHELF_WIDTH]
purpose = "The width shelf wraps plain output at."
type = "port"
unset = "optional"
show = "shown"
source = "setting"
"#;

#[test]
fn an_absent_presence_variable_counts_as_unset_in_its_group() {
    let environment = ready(checked(GROUP, &[]));
    assert_eq!(applied(&environment), None);

    let Verdict::Refused(refusals) = checked(GROUP, &[("SHELF_WIDTH", "80")]) else {
        panic!("the partly set group passed the check");
    };
    assert_eq!(
        refusals.to_string(),
        "NO_COLOR is unset, and the group plain needs it because SHELF_WIDTH is set"
    );
}

#[test]
fn a_presence_variable_set_to_an_empty_value_counts_as_set_in_its_group() {
    let Verdict::Refused(refusals) = checked(GROUP, &[("NO_COLOR", "")]) else {
        panic!("the partly set group passed the check");
    };
    assert_eq!(
        refusals.to_string(),
        "SHELF_WIDTH is unset, and the group plain needs it because NO_COLOR is set"
    );

    ready(checked(GROUP, &[("NO_COLOR", ""), ("SHELF_WIDTH", "80")]));
}

#[test]
fn a_mode_variable_that_reads_its_presence_decides_by_its_raw_value() {
    let text = r#"
[mode]
variable = "SHELF_LIVE"
production = ["live"]

[variables.SHELF_LIVE]
purpose = "Selects production behaviour when it is set to live."
type = "boolean"
presence = true
unset = "optional"
show = "shown"
source = "setting"
"#;
    let spec = Spec::parse(text).unwrap();
    let decided = |pairs: &[(&str, &str)]| {
        let vars = pairs
            .iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)));
        mode(&spec, &capture(&spec, vars))
    };

    assert_eq!(decided(&[("SHELF_LIVE", "live")]), Mode::Production);
    assert_eq!(decided(&[("SHELF_LIVE", "")]), Mode::Local);
    assert_eq!(decided(&[]), Mode::Local);
}
