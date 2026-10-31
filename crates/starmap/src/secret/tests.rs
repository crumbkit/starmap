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
