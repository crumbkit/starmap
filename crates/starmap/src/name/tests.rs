//! Tests for the name of an environment variable.

use proptest::prelude::*;

use super::{Name, NameError};

#[test]
fn a_name_of_upper_case_letters_digits_and_underscores_is_accepted() {
    let name: Name = "SHELF_PORT_2".parse().unwrap();

    assert_eq!(name.as_str(), "SHELF_PORT_2");
}

#[test]
fn a_name_may_start_with_an_underscore() {
    let name: Name = "_CLAP_COMPLETE_INDEX".parse().unwrap();

    assert_eq!(name.to_string(), "_CLAP_COMPLETE_INDEX");
}

#[test]
fn an_empty_name_is_refused() {
    assert_eq!("".parse::<Name>(), Err(NameError::Empty));
}

#[test]
fn a_name_that_starts_with_a_digit_is_refused_at_its_first_character() {
    assert_eq!(
        "2FA_TOKEN".parse::<Name>(),
        Err(NameError::Character {
            character: '2',
            at: 0
        })
    );
}

#[test]
fn a_lower_case_letter_is_refused_at_its_byte_offset() {
    assert_eq!(
        "SHELF_port".parse::<Name>(),
        Err(NameError::Character {
            character: 'p',
            at: 6
        })
    );
}

#[test]
fn a_name_of_exactly_the_length_limit_is_accepted() {
    let name = "A".repeat(Name::MAX_BYTES);

    assert!(name.parse::<Name>().is_ok());
}

#[test]
fn a_name_one_byte_over_the_length_limit_is_refused() {
    let name = "A".repeat(Name::MAX_BYTES + 1);

    assert_eq!(
        name.parse::<Name>(),
        Err(NameError::TooLong {
            bytes: Name::MAX_BYTES + 1
        })
    );
}

#[test]
fn the_errors_say_what_a_name_may_hold() {
    assert_eq!(NameError::Empty.to_string(), "a variable name is empty");
    assert_eq!(
        NameError::TooLong { bytes: 130 }.to_string(),
        "a variable name has 130 bytes, and the limit is 128"
    );
    assert_eq!(
        NameError::Character {
            character: 'p',
            at: 6
        }
        .to_string(),
        "a variable name holds 'p' at byte 6, and a name holds only upper case \
         letters, digits and underscores and does not start with a digit"
    );
}

proptest! {
    #[test]
    fn every_string_that_matches_the_pattern_and_fits_the_limit_is_accepted(
        text in "[A-Z_][A-Z0-9_]{0,127}",
    ) {
        let name: Name = text.parse().unwrap();

        prop_assert_eq!(name.as_str(), text.as_str());
    }

    #[test]
    fn every_string_that_matches_the_pattern_and_exceeds_the_limit_is_refused(
        text in "[A-Z_][A-Z0-9_]{128,300}",
    ) {
        prop_assert_eq!(
            text.parse::<Name>(),
            Err(NameError::TooLong { bytes: text.len() })
        );
    }

    #[test]
    fn every_string_that_starts_with_a_digit_is_refused(
        text in "[0-9][A-Z0-9_]{0,127}",
    ) {
        prop_assert!(text.parse::<Name>().is_err());
    }

    #[test]
    fn every_string_with_a_character_outside_the_pattern_is_refused(
        text in "[A-Z0-9_]{0,20}[^A-Z0-9_][A-Z0-9_]{0,20}",
    ) {
        prop_assert!(text.parse::<Name>().is_err());
    }
}
