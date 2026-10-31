//! Tests for the forms a variable's value takes, and the types that check
//! each one.

use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use proptest::prelude::*;

use super::{
    Choice, ChoiceError, Choices, ChoicesError, Form, FormError, IntegerRange, Port, Value,
};

/// The eight words the boolean form accepts, each with the value it reads as.
const BOOLEAN_WORDS: [(&str, bool); 8] = [
    ("1", true),
    ("0", false),
    ("true", true),
    ("false", false),
    ("yes", true),
    ("no", false),
    ("on", true),
    ("off", false),
];

/// Parses `raw` as `form`.
fn parse(form: &Form, raw: &str) -> Result<Option<Value>, FormError> {
    form.parse(OsStr::new(raw))
}

/// Builds the choices of `values`, which hold no repeat.
fn choices(values: &[&str]) -> Choices {
    Choices::new(values.iter().map(|value| value.parse::<Choice>().unwrap())).unwrap()
}

/// Returns `word` with each letter in upper case where `upper` says so.
fn with_case(word: &str, upper: &[bool]) -> String {
    word.chars()
        .zip(upper.iter().chain(std::iter::repeat(&false)))
        .map(|(letter, &upper)| {
            if upper {
                letter.to_ascii_uppercase()
            } else {
                letter
            }
        })
        .collect()
}

// Port

#[test]
fn a_port_reads_its_number_and_prints_it() {
    let port: Port = "8080".parse().unwrap();

    assert_eq!(port.get(), 8080);
    assert_eq!(u16::from(port), 8080);
    assert_eq!(port.to_string(), "8080");
}

#[test]
fn a_port_is_refused_when_it_is_zero_or_above_65535() {
    assert!("0".parse::<Port>().is_err());
    assert!("65536".parse::<Port>().is_err());
    assert!(Port::try_from(0).is_err());
}

#[test]
fn a_port_is_refused_when_it_holds_a_sign_or_a_space() {
    assert!("+80".parse::<Port>().is_err());
    assert!(" 80".parse::<Port>().is_err());
}

#[test]
fn an_empty_port_is_refused() {
    assert!("".parse::<Port>().is_err());
}

#[test]
fn a_port_with_leading_zeros_reads_its_number() {
    assert_eq!("0080".parse::<Port>().map(Port::get), Ok(80));
}

#[test]
fn a_port_error_says_what_a_port_is() {
    let error = "0".parse::<Port>().unwrap_err();

    assert_eq!(
        error.to_string(),
        "a port is a whole number from 1 to 65535"
    );
}

proptest! {
    #[test]
    fn a_port_number_is_accepted_exactly_from_1_to_65535(number in 0_u32..200_000) {
        let parsed = number.to_string().parse::<Port>();

        prop_assert_eq!(parsed.is_ok(), (1..=65535).contains(&number));
        if let Ok(port) = parsed {
            prop_assert_eq!(port.to_string(), number.to_string());
        }
    }

    #[test]
    fn a_port_converts_from_every_u16_except_zero(number in any::<u16>()) {
        let port = Port::try_from(number);

        prop_assert_eq!(port.is_ok(), number != 0);
        if let Ok(port) = port {
            prop_assert_eq!(port.get(), number);
        }
    }

    #[test]
    fn a_string_of_digits_is_a_port_exactly_when_its_number_is_from_1_to_65535(
        text in "[0-9]{1,7}",
    ) {
        let number: u64 = text.parse().unwrap();
        let parsed = text.parse::<Port>();

        prop_assert_eq!(parsed.is_ok(), (1..=65535).contains(&number));
        if let Ok(port) = parsed {
            prop_assert_eq!(u64::from(port.get()), number);
        }
    }

    #[test]
    fn a_port_with_a_character_other_than_a_digit_is_refused(
        text in "[0-9]{0,3}[^0-9][0-9]{0,3}",
    ) {
        prop_assert!(text.parse::<Port>().is_err());
    }
}

// IntegerRange

#[test]
fn a_range_of_one_value_is_accepted() {
    let range = IntegerRange::new(8, 8).unwrap();

    assert_eq!((range.min(), range.max()), (8, 8));
}

#[test]
fn a_range_whose_minimum_is_above_its_maximum_is_refused() {
    let error = IntegerRange::new(100, 1).unwrap_err();

    assert_eq!((error.min(), error.max()), (100, 1));
    assert_eq!(
        error.to_string(),
        "an integer range's minimum 100 is above its maximum 1"
    );
}

proptest! {
    #[test]
    fn a_range_is_accepted_exactly_when_its_minimum_is_at_most_its_maximum(
        min in any::<i64>(),
        max in any::<i64>(),
    ) {
        prop_assert_eq!(IntegerRange::new(min, max).is_ok(), min <= max);
    }
}

// Choice and Choices

#[test]
fn a_choice_is_refused_when_it_is_empty() {
    assert_eq!("".parse::<Choice>(), Err(ChoiceError::Empty));
}

#[test]
fn a_choice_is_refused_at_its_first_whitespace() {
    assert_eq!(
        "local dev".parse::<Choice>(),
        Err(ChoiceError::Whitespace { at: 5 })
    );
}

#[test]
fn choices_keep_the_order_they_were_given() {
    let choices = choices(&["prod", "dev"]);
    let values: Vec<&str> = choices.iter().map(Choice::as_str).collect();

    assert_eq!(values, ["prod", "dev"]);
}

#[test]
fn choices_are_refused_when_there_are_none() {
    assert_eq!(Choices::new([]), Err(ChoicesError::Empty));
}

#[test]
fn choices_are_refused_at_the_first_repeated_value() {
    let dev: Choice = "dev".parse().unwrap();
    let prod: Choice = "prod".parse().unwrap();

    assert_eq!(
        Choices::new([dev.clone(), prod, dev.clone()]),
        Err(ChoicesError::Repeated(dev))
    );
}

#[test]
fn choices_print_as_a_list() {
    assert_eq!(choices(&["dev", "prod"]).to_string(), "dev, prod");
}

proptest! {
    #[test]
    fn every_string_without_whitespace_is_a_choice(text in "\\S{1,40}") {
        let choice: Choice = text.parse().unwrap();

        prop_assert_eq!(choice.as_str(), text.as_str());
    }

    #[test]
    fn every_string_with_whitespace_is_refused_as_a_choice(text in "\\S{0,10}\\s\\PC{0,10}") {
        let refused = matches!(text.parse::<Choice>(), Err(ChoiceError::Whitespace { .. }));

        prop_assert!(refused);
    }

    #[test]
    fn choices_are_accepted_exactly_when_they_are_not_empty_and_none_repeats(
        values in proptest::collection::vec("[a-c]{1,2}", 0..8),
    ) {
        let parsed = Choices::new(values.iter().map(|value| value.parse::<Choice>().unwrap()));
        let mut unique = values.clone();
        unique.sort();
        unique.dedup();

        prop_assert_eq!(parsed.is_ok(), !values.is_empty() && unique.len() == values.len());
    }
}

// Form::parse for paths

#[test]
fn a_path_form_reads_a_relative_path_when_it_need_not_be_absolute() {
    let form = Form::Path { absolute: false };

    assert_eq!(
        parse(&form, "books.db"),
        Ok(Some(Value::Path(PathBuf::from("books.db"))))
    );
}

#[test]
fn an_absolute_path_form_reads_an_absolute_path() {
    let form = Form::Path { absolute: true };

    assert_eq!(
        parse(&form, "/Users/luke/.local/state"),
        Ok(Some(Value::Path(PathBuf::from("/Users/luke/.local/state"))))
    );
}

#[test]
fn an_absolute_path_form_treats_a_relative_or_empty_path_as_unset() {
    let form = Form::Path { absolute: true };

    assert_eq!(parse(&form, "state"), Ok(None));
    assert_eq!(parse(&form, ""), Ok(None));
}

#[cfg(unix)]
proptest! {
    #[test]
    fn an_absolute_path_rule_returns_unset_exactly_for_relative_paths(
        path in "/?[a-z.]{0,5}(/[a-z.]{0,5}){0,3}",
    ) {
        let read = parse(&Form::Path { absolute: true }, &path);

        if path.starts_with('/') {
            prop_assert_eq!(read, Ok(Some(Value::Path(PathBuf::from(&path)))));
        } else {
            prop_assert_eq!(read, Ok(None));
        }
    }

    #[test]
    fn a_path_form_without_the_absolute_rule_reads_every_path(
        path in "/?[a-z.]{0,5}(/[a-z.]{0,5}){0,3}",
    ) {
        prop_assert_eq!(
            parse(&Form::Path { absolute: false }, &path),
            Ok(Some(Value::Path(PathBuf::from(&path))))
        );
    }
}

// Form::parse for ports, URLs and strings

#[test]
fn a_port_form_reads_a_port() {
    assert_eq!(
        parse(&Form::Port, "443"),
        Ok(Some(Value::Port(Port::try_from(443).unwrap())))
    );
    assert_eq!(parse(&Form::Port, "0"), Err(FormError::NotPort));
}

#[test]
fn a_url_form_reads_a_url() {
    let read = parse(&Form::Url, "https://shelf.example/catalogue");

    assert_eq!(
        read,
        Ok(Some(Value::Url(
            "https://shelf.example/catalogue".parse().unwrap()
        )))
    );
}

#[test]
fn a_url_form_refuses_text_without_a_scheme_and_keeps_the_parse_error_as_its_source() {
    let error = parse(&Form::Url, "shelf.example").unwrap_err();

    assert_eq!(
        error,
        FormError::NotUrl(url::ParseError::RelativeUrlWithoutBase)
    );
    assert_eq!(error.to_string(), "the value is not a url");
    assert!(error.source().is_some());
}

#[test]
fn a_string_form_reads_any_text_including_an_empty_one() {
    assert_eq!(
        parse(&Form::String, ""),
        Ok(Some(Value::String(OsString::new())))
    );
    assert_eq!(
        parse(&Form::String, "two words"),
        Ok(Some(Value::String(OsString::from("two words"))))
    );
}

// Form::parse for booleans

#[test]
fn the_boolean_form_reads_each_of_its_eight_words() {
    for (word, value) in BOOLEAN_WORDS {
        assert_eq!(
            parse(&Form::Boolean, word),
            Ok(Some(Value::Boolean(value))),
            "word {word:?}"
        );
    }
}

#[test]
fn the_boolean_form_refuses_a_word_with_surrounding_space() {
    assert_eq!(parse(&Form::Boolean, " true"), Err(FormError::NotBoolean));
    assert_eq!(parse(&Form::Boolean, "true "), Err(FormError::NotBoolean));
    assert_eq!(parse(&Form::Boolean, ""), Err(FormError::NotBoolean));
}

proptest! {
    #[test]
    fn the_boolean_form_reads_each_word_in_any_mix_of_case(
        index in 0..BOOLEAN_WORDS.len(),
        upper in proptest::collection::vec(any::<bool>(), 5),
    ) {
        let (word, value) = BOOLEAN_WORDS[index];

        prop_assert_eq!(
            parse(&Form::Boolean, &with_case(word, &upper)),
            Ok(Some(Value::Boolean(value)))
        );
    }

    #[test]
    fn the_boolean_form_refuses_a_word_with_any_text_around_it(
        (prefix, suffix) in text_around_a_word(),
        index in 0..BOOLEAN_WORDS.len(),
    ) {
        let text = format!("{prefix}{}{suffix}", BOOLEAN_WORDS[index].0);
        prop_assume!(!BOOLEAN_WORDS.iter().any(|(word, _)| word.eq_ignore_ascii_case(&text)));

        prop_assert_eq!(parse(&Form::Boolean, &text), Err(FormError::NotBoolean));
    }

    #[test]
    fn the_boolean_form_refuses_every_other_string(text in "\\PC{0,6}") {
        prop_assume!(!BOOLEAN_WORDS.iter().any(|(word, _)| word.eq_ignore_ascii_case(&text)));

        prop_assert_eq!(parse(&Form::Boolean, &text), Err(FormError::NotBoolean));
    }
}

// Form::parse for integers

#[test]
fn an_integer_form_reads_a_number_with_an_optional_sign() {
    let form = Form::Integer(IntegerRange::new(-10, 10).unwrap());

    assert_eq!(parse(&form, "-3"), Ok(Some(Value::Integer(-3))));
    assert_eq!(parse(&form, "+3"), Ok(Some(Value::Integer(3))));
}

#[test]
fn an_integer_form_refuses_text_that_is_not_a_whole_number() {
    let form = Form::Integer(IntegerRange::new(1, 100).unwrap());

    assert_eq!(parse(&form, "2.5"), Err(FormError::NotInteger));
    assert_eq!(parse(&form, ""), Err(FormError::NotInteger));
    assert_eq!(parse(&form, "-"), Err(FormError::NotInteger));
}

#[test]
fn an_integer_form_refuses_a_number_outside_its_range_with_the_range() {
    let form = Form::Integer(IntegerRange::new(1, 100).unwrap());
    let error = parse(&form, "0").unwrap_err();

    assert_eq!(error, FormError::OutOfRange { min: 1, max: 100 });
    assert_eq!(
        error.to_string(),
        "the value is not a whole number from 1 to 100"
    );
}

#[test]
fn an_integer_form_refuses_a_number_beyond_64_bits_as_out_of_range() {
    let form = Form::Integer(IntegerRange::new(i64::MIN, i64::MAX).unwrap());

    assert_eq!(
        parse(&form, "9223372036854775808"),
        Err(FormError::OutOfRange {
            min: i64::MIN,
            max: i64::MAX
        })
    );
    assert_eq!(
        parse(&form, "-9223372036854775809"),
        Err(FormError::OutOfRange {
            min: i64::MIN,
            max: i64::MAX
        })
    );
}

/// Generates the text before and after a boolean word, which may hold
/// whitespace and characters outside ASCII. At least one of the two is not
/// empty, and the filter counts as a local reject, so the property's
/// `prop_assume!` stays well under proptest's limit of 1,024 global rejects.
fn text_around_a_word() -> impl Strategy<Value = (String, String)> {
    ("\\PC{0,2}", "\\PC{0,2}").prop_filter("text around the word", |(prefix, suffix)| {
        !prefix.is_empty() || !suffix.is_empty()
    })
}

/// Returns the number that `text` writes when it matches `^[+-]?[0-9]+$`,
/// which holds at most eight digits here, and `None` otherwise.
fn signed_digits(text: &str) -> Option<i64> {
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let magnitude = digits
        .bytes()
        .fold(0_i64, |number, digit| number * 10 + i64::from(digit - b'0'));
    Some(if negative { -magnitude } else { magnitude })
}

/// Generates a range and a number that is often near or inside it, so the
/// property sees both outcomes and both edges.
fn range_and_number() -> impl Strategy<Value = (i64, i64, i64)> {
    (any::<i64>(), any::<i64>()).prop_flat_map(|(first, second)| {
        let (min, max) = (first.min(second), first.max(second));
        let number = prop_oneof![
            any::<i64>(),
            min..=max,
            Just(min),
            Just(max),
            Just(min.saturating_sub(1)),
            Just(max.saturating_add(1)),
        ];
        (Just(min), Just(max), number)
    })
}

proptest! {
    #[test]
    fn an_integer_form_accepts_a_number_exactly_when_it_lies_in_the_range(
        (min, max, number) in range_and_number(),
    ) {
        let form = Form::Integer(IntegerRange::new(min, max).unwrap());
        let read = parse(&form, &number.to_string());

        if (min..=max).contains(&number) {
            prop_assert_eq!(read, Ok(Some(Value::Integer(number))));
        } else {
            prop_assert_eq!(read, Err(FormError::OutOfRange { min, max }));
        }
    }

    #[test]
    fn an_integer_form_reads_text_exactly_when_it_is_one_optional_sign_and_digits(
        text in "[+-]{0,2}[0-9]{0,4}[+-]{0,2}[0-9]{0,4}",
    ) {
        let form = Form::Integer(IntegerRange::new(i64::MIN, i64::MAX).unwrap());

        prop_assert_eq!(parse(&form, &text), match signed_digits(&text) {
            Some(number) => Ok(Some(Value::Integer(number))),
            None => Err(FormError::NotInteger),
        });
    }

    #[test]
    fn an_integer_form_refuses_text_with_a_character_other_than_a_sign_or_digit(
        text in "[0-9]{0,3}[^0-9+-][0-9]{0,3}",
    ) {
        let form = Form::Integer(IntegerRange::new(i64::MIN, i64::MAX).unwrap());

        prop_assert_eq!(parse(&form, &text), Err(FormError::NotInteger));
    }
}

// Form::parse for enumerations

#[test]
fn an_enumeration_form_reads_one_of_its_choices() {
    let form = Form::Enumeration(choices(&["dev", "prod"]));

    assert_eq!(
        parse(&form, "prod"),
        Ok(Some(Value::Choice("prod".parse().unwrap())))
    );
}

#[test]
fn an_enumeration_form_refuses_another_value_and_lists_its_choices() {
    let form = Form::Enumeration(choices(&["dev", "prod"]));
    let error = parse(&form, "Prod").unwrap_err();

    assert_eq!(error, FormError::NotChoice(choices(&["dev", "prod"])));
    assert_eq!(error.to_string(), "the value is not one of dev, prod");
}

proptest! {
    #[test]
    fn an_enumeration_form_accepts_a_value_exactly_when_it_is_a_choice(
        values in proptest::collection::btree_set("[a-c]{1,2}", 1..5),
        text in "[a-c]{0,3}",
    ) {
        let values: Vec<&str> = values.iter().map(String::as_str).collect();
        let form = Form::Enumeration(choices(&values));

        prop_assert_eq!(parse(&form, &text).is_ok(), values.contains(&text.as_str()));
    }
}

// Form::parse for text that is not Unicode

#[cfg(unix)]
#[test]
fn every_form_that_needs_text_refuses_a_value_that_is_not_unicode() {
    use std::os::unix::ffi::OsStrExt;

    let raw = OsStr::from_bytes(b"\xff80");
    let forms = [
        Form::Port,
        Form::Url,
        Form::Boolean,
        Form::Integer(IntegerRange::new(1, 100).unwrap()),
        Form::Enumeration(choices(&["dev"])),
    ];

    for form in forms {
        assert_eq!(form.parse(raw), Err(FormError::NotUnicode), "form {form:?}");
    }
}

#[cfg(unix)]
#[test]
fn the_string_and_path_forms_read_a_value_that_is_not_unicode() {
    use std::os::unix::ffi::OsStrExt;

    let raw = OsStr::from_bytes(b"/state/\xff");

    assert_eq!(
        Form::String.parse(raw),
        Ok(Some(Value::String(raw.to_os_string())))
    );
    assert_eq!(
        Form::Path { absolute: true }.parse(raw),
        Ok(Some(Value::Path(PathBuf::from(raw))))
    );
}
