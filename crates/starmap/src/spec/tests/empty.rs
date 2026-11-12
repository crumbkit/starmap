//! Tests for the parser's reading of a variable's `empty` field, and of
//! the canonical form it writes.

use super::{kinds, name, one, spec};
use crate::{Empty, Spec, SpecProblemKind};

/// Returns a spec with one shown setting `SHELF_X` whose type is `ty`, whose
/// unset rule is `unset`, and that has the extra field `extra`.
fn setting(ty: &str, unset: &str, extra: &str) -> String {
    one(&format!(
        "purpose = \"How many catalogue workers run.\"\ntype = {ty}\nunset = {unset}\n\
         show = \"shown\"\nsource = \"setting\"\n{extra}"
    ))
}

/// Returns what an empty value of the one variable `SHELF_X` in `text`, a
/// valid spec, counts as.
fn empty(text: &str) -> Empty {
    spec(text).variable(&name("SHELF_X")).unwrap().empty()
}

#[test]
fn an_empty_value_counts_as_unset_without_the_field_or_when_it_is_unset() {
    let without = setting(
        "{ integer = { min = 1, max = 64 } }",
        "{ default = \"5\" }",
        "",
    );
    let unset = setting(
        "{ integer = { min = 1, max = 64 } }",
        "{ default = \"5\" }",
        "empty = \"unset\"",
    );

    assert_eq!(empty(&without), Empty::Unset);
    assert_eq!(empty(&unset), Empty::Unset);
    assert_eq!(spec(&without).digest(), spec(&unset).digest());
}

#[test]
fn an_empty_value_is_a_value_when_the_field_is_value() {
    let text = setting(
        "{ integer = { min = 1, max = 64 } }",
        "{ default = \"5\" }",
        "empty = \"value\"",
    );

    assert_eq!(empty(&text), Empty::Value);
}

#[test]
fn a_variable_that_reads_its_presence_counts_an_empty_value_as_a_value() {
    let text = one(
        "purpose = \"Turns colour off when it is set.\"\ntype = \"boolean\"\npresence = true\n\
         unset = \"optional\"\nshow = \"shown\"\nsource = \"inherited\"",
    );

    assert_eq!(empty(&text), Empty::Value);
}

#[test]
fn an_empty_field_with_another_word_is_refused_with_the_words_it_takes() {
    let text = setting(
        "{ integer = { min = 1, max = 64 } }",
        "\"optional\"",
        "empty = \"blank\"",
    );

    let error = Spec::parse(&text).unwrap_err();

    assert_eq!(error.problems().len(), 1);
    assert_eq!(error.problems()[0].kind(), SpecProblemKind::BadValue);
    assert_eq!(
        error.problems()[0].to_string(),
        "`variables.SHELF_X.empty` is \"blank\", and it takes one of \"unset\" and \"value\""
    );
}

#[test]
fn an_empty_field_beside_presence_is_refused_with_the_rule() {
    for word in ["value", "unset"] {
        let text = one(&format!(
            "purpose = \"Turns colour off when it is set.\"\ntype = \"boolean\"\n\
             presence = true\nempty = \"{word}\"\nunset = \"optional\"\nshow = \"shown\"\n\
             source = \"inherited\""
        ));

        let error = Spec::parse(&text).unwrap_err();

        assert_eq!(error.problems().len(), 1, "{word}");
        assert_eq!(error.problems()[0].kind(), SpecProblemKind::BadValue);
        assert_eq!(
            error.problems()[0].to_string(),
            format!(
                "`variables.SHELF_X.empty` is \"{word}\", and a variable that reads its \
                 presence already counts an empty value as set, so it takes no `empty`"
            )
        );
    }
}

#[test]
fn a_family_takes_the_empty_field() {
    let text = "[families.SHELF_WORKERS]\npurpose = \"How many workers one region runs.\"\n\
                type = { integer = { min = 1, max = 64 } }\nunset = \"optional\"\nshow = \"shown\"\n\
                source = \"setting\"\nempty = \"value\"\n";

    let spec = spec(text);
    let family = spec.family(&name("SHELF_WORKERS")).unwrap();

    assert_eq!(family.variable().empty(), Empty::Value);
    assert!(
        spec.to_toml().contains("empty = \"value\""),
        "{}",
        spec.to_toml()
    );
}

#[test]
fn the_canonical_form_writes_empty_after_the_type_only_when_it_is_value() {
    let value = spec(&setting("\"string\"", "\"optional\"", "empty = \"value\""));
    let unset = spec(&setting("\"string\"", "\"optional\"", "empty = \"unset\""));

    let canonical = value.to_toml();

    assert!(
        canonical.contains("type = \"string\"\nempty = \"value\"\nunset = \"optional\"\n"),
        "{canonical}"
    );
    assert_eq!(spec(&canonical), value);
    assert!(!unset.to_toml().contains("empty"));
    assert_ne!(value.digest(), unset.digest());
}

#[test]
fn an_empty_field_that_is_not_text_is_refused() {
    let text = setting("\"string\"", "\"optional\"", "empty = true");

    assert_eq!(kinds(&text), [SpecProblemKind::BadValue]);
}
