//! Tests for the parser's reading of a variable's `presence` field.

use super::{kinds, name, one, spec};
use crate::{Spec, SpecProblemKind};

/// Returns a spec with one shown inherited variable whose type is `ty`,
/// whose unset rule is `unset`, and that has the extra field `extra`.
fn inherited(ty: &str, unset: &str, extra: &str) -> String {
    one(&format!(
        "purpose = \"Turns colour off when it is set.\"\ntype = {ty}\nunset = {unset}\n\
         show = \"shown\"\nsource = \"inherited\"\n{extra}"
    ))
}

/// Whether the one variable `SHELF_X` in `text`, a valid spec, reads its
/// presence.
fn presence(text: &str) -> bool {
    spec(text).variable(&name("SHELF_X")).unwrap().presence()
}

#[test]
fn a_boolean_reads_its_presence_when_the_field_is_true() {
    let text = inherited("\"boolean\"", "\"optional\"", "presence = true");

    assert!(presence(&text));
}

#[test]
fn a_variable_does_not_read_its_presence_without_the_field_or_when_it_is_false() {
    let without = inherited("\"boolean\"", "\"optional\"", "");
    let off = inherited("\"boolean\"", "\"optional\"", "presence = false");

    assert!(!presence(&without));
    assert!(!presence(&off));
    assert_eq!(spec(&without).digest(), spec(&off).digest());
}

#[test]
fn a_presence_that_is_not_true_or_false_is_a_bad_value() {
    let text = inherited("\"boolean\"", "\"optional\"", "presence = \"yes\"");

    assert_eq!(kinds(&text), [SpecProblemKind::BadValue]);
}

#[test]
fn presence_beside_a_type_other_than_boolean_is_refused_with_the_rule() {
    let text = inherited("\"string\"", "\"optional\"", "presence = true");

    let error = Spec::parse(&text).unwrap_err();

    assert_eq!(error.problems().len(), 1);
    assert_eq!(error.problems()[0].kind(), SpecProblemKind::BadValue);
    assert_eq!(
        error.problems()[0].to_string(),
        "`variables.SHELF_X.presence` is true, and only a variable with the type \"boolean\" \
         reads its presence"
    );
}

#[test]
fn presence_beside_declared_spellings_is_refused_with_the_rule() {
    let ty = "{ boolean = { true = [\"1\"], false = [\"0\"] } }";
    let text = inherited(ty, "\"optional\"", "presence = true");

    let error = Spec::parse(&text).unwrap_err();

    assert_eq!(error.problems().len(), 1);
    assert_eq!(error.problems()[0].kind(), SpecProblemKind::BadValue);
    assert_eq!(
        error.problems()[0].to_string(),
        "`variables.SHELF_X.presence` is true, and a variable that reads its presence takes \
         any value, so its type declares no spellings"
    );
}

#[test]
fn presence_beside_a_default_is_refused_with_the_rule() {
    let text = inherited("\"boolean\"", "{ default = \"false\" }", "presence = true");

    let error = Spec::parse(&text).unwrap_err();

    assert_eq!(error.problems().len(), 1);
    assert_eq!(error.problems()[0].kind(), SpecProblemKind::BadValue);
    assert_eq!(
        error.problems()[0].to_string(),
        "`variables.SHELF_X.presence` is true, and a variable that reads its presence is \
         false when it is unset, so it takes no default"
    );
}

#[test]
fn a_false_presence_is_allowed_beside_any_type_and_a_default() {
    let text = inherited("\"port\"", "{ default = \"8080\" }", "presence = false");

    assert!(!presence(&text));
}

#[test]
fn a_family_does_not_take_presence() {
    let text = "[families.SHELF_FLAG]\npurpose = \"One flag per region.\"\ntype = \"boolean\"\n\
                unset = \"optional\"\nshow = \"shown\"\nsource = \"setting\"\npresence = true\n";

    assert_eq!(kinds(text), [SpecProblemKind::UnknownField]);
}

#[test]
fn the_canonical_form_writes_presence_only_when_it_is_true_and_parses_back_to_the_same_spec() {
    let original = spec(&inherited("\"boolean\"", "\"optional\"", "presence = true"));
    let plain = spec(&inherited("\"boolean\"", "\"optional\"", ""));

    let canonical = original.to_toml();

    assert!(
        canonical.contains("type = \"boolean\"\npresence = true\nunset = \"optional\"\n"),
        "{canonical}"
    );
    assert_eq!(spec(&canonical), original);
    assert!(!plain.to_toml().contains("presence"));
    assert_ne!(original.digest(), plain.digest());
}
