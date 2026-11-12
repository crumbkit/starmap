//! Tests for the spellings a boolean type declares.

use super::{Spellings, SpellingsError};

/// Builds spellings from two lists of text.
fn spellings(for_true: &[&str], for_false: &[&str]) -> Result<Spellings, SpellingsError> {
    let owned = |list: &[&str]| list.iter().map(|&text| text.to_owned()).collect::<Vec<_>>();
    Spellings::new(owned(for_true), owned(for_false))
}

#[test]
fn spellings_keep_both_lists_in_order() {
    let spellings = spellings(&["1", "yes"], &["0", "no"]).unwrap();

    assert_eq!(spellings.for_true(), ["1", "yes"]);
    assert_eq!(spellings.for_false(), ["0", "no"]);
    assert_eq!(spellings.to_string(), "1, yes for true and 0, no for false");
}

#[test]
fn a_value_matches_a_spelling_exactly() {
    let spellings = spellings(&["1", "yes"], &["0", "no"]).unwrap();

    assert_eq!(spellings.read("yes"), Some(true));
    assert_eq!(spellings.read("0"), Some(false));
    for other in ["YES", "true", " 1", "no ", ""] {
        assert_eq!(spellings.read(other), None, "{other:?}");
    }
}

#[test]
fn a_list_without_a_spelling_is_refused() {
    assert_eq!(spellings(&[], &["0"]), Err(SpellingsError::Empty(true)));
    assert_eq!(spellings(&["1"], &[]), Err(SpellingsError::Empty(false)));
}

#[test]
fn an_empty_spelling_is_refused() {
    assert_eq!(
        spellings(&["1", ""], &["0"]),
        Err(SpellingsError::EmptySpelling)
    );
    assert_eq!(spellings(&["1"], &[""]), Err(SpellingsError::EmptySpelling));
}

#[test]
fn a_spelling_that_appears_twice_is_refused() {
    let repeated = |text: &str| Err(SpellingsError::Repeated(text.to_owned()));

    assert_eq!(spellings(&["1", "1"], &["0"]), repeated("1"));
    assert_eq!(spellings(&["1"], &["0", "0"]), repeated("0"));
    assert_eq!(spellings(&["1", "on"], &["0", "on"]), repeated("on"));
}

#[test]
fn each_refusal_says_what_the_lists_break() {
    assert_eq!(
        SpellingsError::Empty(false).to_string(),
        "a boolean lists no spellings for false"
    );
    assert_eq!(
        SpellingsError::EmptySpelling.to_string(),
        "a boolean lists an empty spelling"
    );
    assert_eq!(
        SpellingsError::Repeated("on".to_owned()).to_string(),
        "a boolean lists the spelling \"on\" twice"
    );
}
