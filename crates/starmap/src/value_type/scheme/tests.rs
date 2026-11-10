//! Tests for the schemes a url type allows.

use super::{Scheme, SchemeError, Schemes, SchemesError};

/// Parses each of `words` as a scheme.
fn schemes(words: &[&str]) -> Vec<Scheme> {
    words.iter().map(|word| word.parse().unwrap()).collect()
}

#[test]
fn a_scheme_of_a_letter_then_letters_digits_plus_minus_and_dot_is_accepted() {
    for word in ["https", "postgres", "svn+ssh", "x-custom", "coap.tcp", "h2"] {
        let scheme: Scheme = word.parse().unwrap();

        assert_eq!(scheme.as_str(), word);
    }
}

#[test]
fn a_scheme_that_is_empty_starts_with_another_character_or_holds_upper_case_is_refused() {
    for word in [
        "",
        "2http",
        "+ssh",
        "HTTPS",
        "Postgres",
        "http:",
        "my scheme",
    ] {
        assert_eq!(word.parse::<Scheme>(), Err(SchemeError(())), "{word:?}");
    }
}

#[test]
fn a_scheme_error_states_the_rule() {
    assert_eq!(
        SchemeError(()).to_string(),
        "a scheme is a lower case ascii letter followed by lower case letters, digits, +, - \
         and ."
    );
}

#[test]
fn schemes_keep_the_order_they_were_given_and_print_as_a_list() {
    let list = Schemes::new(schemes(&["postgresql", "postgres"])).unwrap();

    let words: Vec<&str> = list.iter().map(Scheme::as_str).collect();
    assert_eq!(words, ["postgresql", "postgres"]);
    assert_eq!(list.to_string(), "postgresql, postgres");
}

#[test]
fn schemes_are_refused_when_there_are_none_or_one_repeats() {
    assert_eq!(Schemes::new(Vec::new()), Err(SchemesError::Empty));
    assert_eq!(
        Schemes::new(schemes(&["https", "http", "https"])),
        Err(SchemesError::Repeated("https".parse().unwrap()))
    );
    assert_eq!(
        SchemesError::Empty.to_string(),
        "a url's scheme list has no schemes"
    );
    assert_eq!(
        SchemesError::Repeated("https".parse().unwrap()).to_string(),
        "a url's scheme list lists the scheme https twice"
    );
}

#[test]
fn schemes_say_whether_they_hold_a_scheme() {
    let list = Schemes::new(schemes(&["https"])).unwrap();

    assert!(list.contains("https"));
    assert!(!list.contains("http"));
}
