//! Tests for reading `.env` beside the spec.

use std::path::Path;

use crumbkit::{Failure as _, Recovery};
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
fn a_member_of_a_family_is_read_and_a_name_outside_it_is_listed() {
    let spec = Spec::parse(
        r#"
[families.SHELF_DB_URL]
purpose = "The database of one region, keyed by the region's code."
type = "url"
unset = "optional"
show = "shown"
source = "setting"
"#,
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(".env");
    std::fs::write(
        &file,
        "SHELF_DB_URL_EU=https://eu.example/\nSHELF_DB_URLS=x\n",
    )
    .unwrap();

    let dotenv = read(&file, &spec).unwrap();

    assert_eq!(
        dotenv.values,
        [("SHELF_DB_URL_EU".into(), "https://eu.example/".into())]
    );
    assert_eq!(dotenv.undeclared, ["SHELF_DB_URLS"]);
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

#[test]
fn an_unreadable_file_offers_to_make_it_readable_or_to_remove_it() {
    let mut text = b"SHELF_DEBUG=".to_vec();
    text.resize(MAX_BYTES + 1, b'x');
    let failures = read_text(&text).unwrap_err();
    let [failure @ DotEnvFailure::Unreadable { file, .. }] = failures.as_slice() else {
        panic!("{failures:?}");
    };
    assert_eq!(
        failure.recovery(),
        Recovery::hint(format!(
            "make {file} a readable text file of at most 256 KiB"
        ))
    );
    assert_eq!(
        failure.alternatives(),
        [Recovery::hint(format!("remove {file}"))]
    );
}

/// Returns the value that `.env` holding `line` gives `SHELF_DEBUG`.
fn debug_value(line: &str) -> Result<String, Vec<DotEnvFailure>> {
    let dotenv = read_text(format!("# first\n{line}\n").as_bytes())?;
    let [(_, value)] = dotenv.values.as_slice() else {
        panic!("{line}: {:?}", dotenv.values);
    };
    Ok(value.to_string_lossy().into_owned())
}

#[test]
fn each_accepted_form_gives_its_value() {
    let cases = [
        ("SHELF_DEBUG=true", "true"),
        ("export SHELF_DEBUG=true", "true"),
        ("  SHELF_DEBUG = true  ", "true"),
        ("SHELF_DEBUG=true # a comment", "true"),
        ("SHELF_DEBUG=true\t# a comment", "true"),
        ("SHELF_DEBUG=a#b", "a#b"),
        ("SHELF_DEBUG='a b # c'", "a b # c"),
        ("SHELF_DEBUG=\"a b # c\" # a comment", "a b # c"),
        ("SHELF_DEBUG=", ""),
        ("SHELF_DEBUG=$HOME", "$HOME"),
        ("SHELF_DEBUG='${HOME}'", "${HOME}"),
        ("SHELF_DEBUG=\"pre-$HOME\"", "pre-$HOME"),
        ("SHELF_DEBUG=a\\b", "a\\b"),
    ];
    for (line, expected) in cases {
        assert_eq!(debug_value(line).unwrap(), expected, "{line}");
    }
}

#[test]
fn blank_lines_and_comments_give_nothing() {
    let dotenv = read_text(b"\n   \n# SHELF_DEBUG=true\n  # indented\n").unwrap();
    assert!(dotenv.values.is_empty());
    assert!(dotenv.undeclared.is_empty());
}

#[test]
fn each_refused_line_names_its_number_only_and_its_twin_is_accepted() {
    let cases = [
        ("SHELF_DEBUG true", "SHELF_DEBUG=true"),
        ("SHELF-DEBUG=true", "SHELF_DEBUG=true"),
        ("=true", "SHELF_DEBUG=true"),
        ("SHELF_DEBUG='multi", "SHELF_DEBUG='multi'"),
        ("SHELF_DEBUG=\"a\\nb\"", "SHELF_DEBUG=\"anb\""),
        ("SHELF_DEBUG='a' b", "SHELF_DEBUG='a b'"),
        ("SHELF_DEBUG=it's", "SHELF_DEBUG=\"it's\""),
    ];
    for (refused, admitted) in cases {
        let failures = debug_value(refused).unwrap_err();
        let [DotEnvFailure::Syntax { at }] = failures.as_slice() else {
            panic!("{refused}: {failures:?}");
        };
        assert!(at.ends_with(".env:2"), "{refused}: {at}");
        assert!(
            !failures[0].to_string().contains("SHELF_DEBUG"),
            "{refused}"
        );
        assert!(debug_value(admitted).is_ok(), "{admitted}");
    }
}
