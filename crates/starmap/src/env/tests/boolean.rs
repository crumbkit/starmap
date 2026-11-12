//! Tests for how the checked environment holds a boolean that declares its
//! spellings.

use std::ffi::OsString;

use super::{changes, command, name};
use crate::field::FieldType;
use crate::{Environment, Mode, Spec, Verdict, capture, check};

/// A spec with a boolean that is spelled `1` or `yes` for true and `0` or
/// `no` for false, and defaults to `0`.
const SPELLED: &str = r#"
[variables.SHELF_DEBUG]
purpose = "Logs each catalogue query."
type = { boolean = { true = ["1", "yes"], false = ["0", "no"] } }
unset = { default = "0" }
show = "shown"
source = "setting"
"#;

/// Checks `pairs` against the spec `text` in local mode.
pub(super) fn checked(text: &str, pairs: &[(&str, &str)]) -> Verdict {
    let spec = Spec::parse(text).unwrap();
    let vars = pairs
        .iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)));
    check(&spec, &capture(&spec, vars), Mode::Local)
}

/// Returns the environment of a verdict that is ready.
pub(super) fn ready(verdict: Verdict) -> Environment {
    match verdict {
        Verdict::Ready(environment) => environment,
        Verdict::Refused(refusals) => panic!("the check refused: {refusals:?}"),
    }
}

/// Returns what `environment` sets `SHELF_DEBUG` to in a child process.
pub(super) fn applied(environment: &Environment) -> Option<String> {
    let mut command = command();
    environment.apply(&mut command);
    changes(&command).remove("SHELF_DEBUG").flatten()
}

#[test]
fn a_spelled_boolean_reads_as_a_bool_and_reaches_the_child_in_the_spelling_it_was_set_in() {
    let debug = name("SHELF_DEBUG");
    for (set, value) in [("1", true), ("yes", true), ("0", false), ("no", false)] {
        let environment = ready(checked(SPELLED, &[("SHELF_DEBUG", set)]));

        assert_eq!(bool::read(&environment, &debug), Ok(value), "{set}");
        assert_eq!(applied(&environment).as_deref(), Some(set));
    }
}

#[test]
fn a_spelled_boolean_takes_its_default_in_the_spelling_the_file_writes() {
    let environment = ready(checked(SPELLED, &[]));

    assert_eq!(bool::read(&environment, &name("SHELF_DEBUG")), Ok(false));
    assert_eq!(applied(&environment).as_deref(), Some("0"));
}

#[test]
fn a_spelled_boolean_refuses_a_word_it_does_not_list() {
    for other in ["true", "YES", "on"] {
        let Verdict::Refused(refusals) = checked(SPELLED, &[("SHELF_DEBUG", other)]) else {
            panic!("{other} passed the check");
        };

        assert!(
            refusals.to_string().contains(
                "SHELF_DEBUG: the value is not one of 1, yes for true and 0, no for false"
            ),
            "{refusals}"
        );
    }
}
