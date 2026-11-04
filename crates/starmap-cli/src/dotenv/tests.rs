//! Tests for reading `.env` beside the spec.

use std::path::Path;

use starmap::Spec;

use super::{DotEnvFailure, MAX_BYTES, read};

const SPEC: &str = r#"
[variables.SHELF_DEBUG]
purpose = "Logs each catalogue query."
type = "boolean"
unset = { default = "false" }
show = "shown"
source = "setting"

[variables.SHELF_SESSION_KEY]
purpose = "Signs the cookies of the web view."
type = "string"
unset = "required"
show = "withheld"
source = "generated"
"#;

fn spec() -> Spec {
    Spec::parse(SPEC).unwrap()
}

/// Writes `text` as `.env` in a new directory and reads it.
fn read_text(text: &[u8]) -> Result<super::DotEnv, Vec<DotEnvFailure>> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(".env");
    std::fs::write(&file, text).unwrap();
    read(&file, &spec())
}

#[test]
fn a_missing_file_gives_no_values() {
    let dotenv = read(Path::new("/no/such/dir/.env"), &spec()).unwrap();
    assert!(dotenv.values.is_empty());
    assert!(dotenv.undeclared.is_empty());
}

#[test]
fn a_declared_setting_is_read_and_an_undeclared_key_is_listed() {
    let dotenv = read_text(b"# a comment\nSHELF_DEBUG=true\nSHELF_THEME='dark'\n").unwrap();
    assert_eq!(dotenv.values, [("SHELF_DEBUG".into(), "true".into())]);
    assert_eq!(dotenv.undeclared, ["SHELF_THEME"]);
}

#[test]
fn a_generated_variable_in_the_file_is_refused() {
    let failures = read_text(b"SHELF_DEBUG=true\nSHELF_SESSION_KEY=a-test-value\n").unwrap_err();
    assert!(matches!(
        failures.as_slice(),
        [DotEnvFailure::Secret { name, .. }] if name.as_str() == "SHELF_SESSION_KEY"
    ));
}

#[test]
fn a_line_that_does_not_parse_is_refused_with_its_number_and_without_its_text() {
    let failures = read_text(b"SHELF_DEBUG=true\ngarbled entry\n").unwrap_err();
    let [DotEnvFailure::Syntax { at }] = failures.as_slice() else {
        panic!("{failures:?}");
    };
    assert!(at.ends_with(".env:2"), "{at}");
    assert!(!failures[0].to_string().contains("garbled"));
}

#[test]
fn a_file_over_the_limit_is_refused_as_unreadable() {
    let mut text = b"SHELF_DEBUG=".to_vec();
    text.resize(MAX_BYTES + 1, b'x');
    let failures = read_text(&text).unwrap_err();
    assert!(matches!(
        failures.as_slice(),
        [DotEnvFailure::Unreadable { .. }]
    ));
}
