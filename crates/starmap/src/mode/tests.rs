//! Tests for the mode rule.

use std::ffi::OsStr;

use proptest::prelude::*;

use super::{Mode, ModeRule};
use crate::{Choice, Choices, Name};

/// Returns the rule of the example spec: `SHELF_ENV` selects production
/// when it is `prod` or `live`.
fn rule() -> ModeRule {
    let variable: Name = "SHELF_ENV".parse().unwrap();
    let production = ["prod", "live"].map(|value| value.parse::<Choice>().unwrap());
    ModeRule::new(variable, Choices::new(production).unwrap())
}

#[test]
fn the_rule_names_its_variable() {
    assert_eq!(rule().variable().as_str(), "SHELF_ENV");
}

#[test]
fn the_rule_lists_its_production_values_in_order() {
    let rule = rule();
    let values: Vec<&str> = rule.production().iter().map(Choice::as_str).collect();

    assert_eq!(values, ["prod", "live"]);
}

#[test]
fn an_unset_mode_variable_selects_local_mode() {
    assert_eq!(rule().decide(None), Mode::Local);
}

#[test]
fn each_production_value_selects_production_mode() {
    assert_eq!(rule().decide(Some(OsStr::new("prod"))), Mode::Production);
    assert_eq!(rule().decide(Some(OsStr::new("live"))), Mode::Production);
}

#[test]
fn a_production_value_in_another_case_selects_local_mode() {
    assert_eq!(rule().decide(Some(OsStr::new("PROD"))), Mode::Local);
}

#[test]
fn a_mode_prints_its_name_in_lower_case() {
    assert_eq!(Mode::Local.to_string(), "local");
    assert_eq!(Mode::Production.to_string(), "production");
}

proptest! {
    #[test]
    fn every_value_outside_the_production_list_selects_local_mode(value in ".*") {
        prop_assume!(value != "prod" && value != "live");

        prop_assert_eq!(rule().decide(Some(OsStr::new(&value))), Mode::Local);
    }
}
