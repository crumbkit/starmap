use std::ffi::OsString;

use starmap::{Mode, Spec};

use super::decide;

/// A spec whose mode variable is `STAGE`, with production value `prod`, and
/// whose default is `dev` when `default` is true.
fn spec(default: bool) -> Spec {
    let unset = if default {
        "{ default = \"dev\" }"
    } else {
        "\"optional\""
    };
    Spec::parse(&format!(
        r#"
[mode]
variable = "STAGE"
production = ["prod"]

[variables.STAGE]
purpose = "Selects the mode."
type = {{ enumeration = ["dev", "prod"] }}
unset = {unset}
show = "shown"
source = "setting"
"#
    ))
    .unwrap()
}

/// Returns the mode and note that `vars` decide for `spec`.
fn decided(spec: &Spec, vars: &[(&str, &str)]) -> (Mode, String) {
    let vars = vars
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)));
    decide(spec, &starmap::capture(spec, vars))
}

#[test]
fn a_production_value_decides_production_and_the_note_names_it() {
    assert_eq!(
        decided(&spec(true), &[("STAGE", "prod")]),
        (
            Mode::Production,
            "Mode: production, because STAGE is prod.".to_owned()
        )
    );
}

#[test]
fn an_unset_mode_variable_names_its_default() {
    assert_eq!(
        decided(&spec(true), &[]).1,
        "Mode: local, because STAGE is unset and its default is dev."
    );
}

#[test]
fn an_unset_mode_variable_without_a_default_is_named_as_unset() {
    assert_eq!(
        decided(&spec(false), &[]).1,
        "Mode: local, because STAGE is unset."
    );
}

#[test]
fn a_value_that_does_not_fit_decides_local_and_says_so() {
    assert_eq!(
        decided(&spec(true), &[("STAGE", "staging")]),
        (
            Mode::Local,
            "Mode: local, because the value of STAGE does not fit its type.".to_owned()
        )
    );
}

#[test]
fn a_spec_without_a_mode_table_is_local() {
    let spec = Spec::parse("").unwrap();
    assert_eq!(
        decided(&spec, &[]).1,
        "Mode: local, because the spec has no [mode] table."
    );
}
