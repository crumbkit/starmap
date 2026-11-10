//! Tests for the secret wrappers.

use super::Secret;
use crate::{ByteCount, ByteLength, ByteRule, Encoding, TypeError};

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

/// Builds the rule of `length` bytes written in `encoding`.
fn rule(encoding: Encoding, length: u16) -> ByteRule {
    ByteRule::new(
        encoding,
        ByteLength::Exactly(ByteCount::new(length).unwrap()),
    )
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test checks the bytes that the holder returns"
)]
fn a_secret_decodes_into_bytes_held_as_a_secret() {
    let secret = Secret::new("3q2+7w==".to_owned());

    let bytes = secret.decode(rule(Encoding::Base64, 4)).unwrap();

    assert_eq!(bytes.expose(), [0xde, 0xad, 0xbe, 0xef]);
    assert_eq!(bytes.duplicate().expose(), [0xde, 0xad, 0xbe, 0xef]);
    assert_eq!(format!("{bytes:?}"), "SecretBytes(••••••••)");
}

#[test]
fn a_secret_that_does_not_decode_is_refused_without_its_text() {
    let secret = Secret::new("hunter2".to_owned());

    let error = secret.decode(rule(Encoding::Hex, 4)).unwrap_err();

    assert_eq!(error, TypeError::NotEncoded(Encoding::Hex));
    assert!(!error.to_string().contains("hunter2"));
}
