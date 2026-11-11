//! Tests for the value of a family that a field reads.

use std::ffi::OsString;

use url::Url;

use super::read_family;
use crate::{Environment, FieldProblem, Mode, Secret, Spec, Verdict, capture, check};

/// A spec with the open family `SHELF_DB_URL` of urls and the open family
/// `SHELF_TOKEN` of withheld strings.
const SPEC: &str = r#"
[families.SHELF_DB_URL]
purpose = "The database of one region, keyed by the region's code."
type = "url"
unset = "optional"
show = "shown"
source = "setting"

[families.SHELF_TOKEN]
purpose = "The API token of one tenant."
type = "string"
unset = "optional"
show = "withheld"
source = "setting"
"#;

/// Returns the environment that `vars` give [`SPEC`].
fn environment(vars: &[(&str, &str)]) -> Environment {
    let spec = Spec::parse(SPEC).unwrap();
    let vars = vars
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)));
    match check(&spec, &capture(&spec, vars), Mode::Local) {
        Verdict::Ready(environment) => environment,
        Verdict::Refused(refusals) => panic!("the check refused: {refusals}"),
    }
}

#[test]
fn a_family_holds_each_set_member_under_its_key_in_key_order() {
    let environment = environment(&[
        ("SHELF_DB_URL_US", "https://us.example/"),
        ("SHELF_DB_URL_EU", "https://eu.example/"),
    ]);

    let family = read_family::<Url>(&environment, "SHELF_DB_URL").unwrap();

    assert_eq!(family.len(), 2);
    assert!(!family.is_empty());
    assert_eq!(family.keys().collect::<Vec<_>>(), ["EU", "US"]);
    assert_eq!(
        family.get("EU").map(Url::as_str),
        Some("https://eu.example/")
    );
    assert_eq!(family.get("APAC"), None);
    let iterated: Vec<(&str, &str)> = family
        .iter()
        .map(|(key, url)| (key, url.as_str()))
        .collect();
    assert_eq!(
        iterated,
        [("EU", "https://eu.example/"), ("US", "https://us.example/")]
    );
}

#[test]
fn a_family_with_no_member_set_is_empty() {
    let environment = environment(&[("SHELF_DB_URL_EU", "")]);

    let family = read_family::<Url>(&environment, "SHELF_DB_URL").unwrap();

    assert!(family.is_empty());
    assert_eq!(family.len(), 0);
}

#[test]
fn a_withheld_family_holds_each_member_as_a_secret() {
    let environment = environment(&[("SHELF_TOKEN_ACME", "s3cret")]);

    let family = read_family::<Secret>(&environment, "SHELF_TOKEN").unwrap();

    assert_eq!(family.keys().collect::<Vec<_>>(), ["ACME"]);
    assert!(!format!("{family:?}").contains("s3cret"));
}

#[test]
fn a_family_read_into_a_type_that_cannot_hold_its_value_is_a_disagreement() {
    let environment = environment(&[("SHELF_DB_URL_EU", "https://eu.example/")]);

    let error = read_family::<bool>(&environment, "SHELF_DB_URL").unwrap_err();

    assert_eq!(error.name(), "SHELF_DB_URL");
    assert_eq!(error.problem(), FieldProblem::Disagreement);
}
