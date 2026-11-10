//! Tests for the secret wrapper.

use super::Secret;

#[test]
fn a_secret_prints_eight_dots_in_debug() {
    let secret = Secret::new("hunter2".to_owned());

    assert_eq!(format!("{secret:?}"), "Secret(••••••••)");
}

#[test]
fn a_secret_prints_the_same_dots_whatever_its_length() {
    let short = Secret::new("a".to_owned());
    let long = Secret::new("a much longer secret value".to_owned());

    assert_eq!(format!("{short:?}"), format!("{long:?}"));
    assert_eq!(format!("{short:#?}"), "Secret(••••••••)");
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test checks the value that expose returns"
)]
fn expose_returns_the_value_the_secret_was_built_with() {
    let secret = Secret::new("hunter2".to_owned());

    assert_eq!(secret.expose(), "hunter2");
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test checks the value that the copy holds"
)]
fn a_duplicate_holds_the_value_of_the_secret_it_copies() {
    let secret = Secret::new("hunter2".to_owned());

    assert_eq!(secret.duplicate().expose(), "hunter2");
}

#[test]
fn a_secret_that_does_not_match_a_pattern_is_refused_by_the_pattern_alone() {
    let pattern: crate::Pattern = "sk-[a-z]+".parse().unwrap();

    assert_eq!(Secret::new("sk-abc".to_owned()).check(&pattern), Ok(()));
    assert_eq!(
        Secret::new("pk-hidden".to_owned()).check(&pattern),
        Err(crate::TypeError::NoMatch(pattern))
    );
}
