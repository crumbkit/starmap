//! Tests for the parser's reading of a boolean type's spellings.

use std::ffi::OsStr;

use super::{kinds, name, one, spec};
use crate::{Spec, SpecProblemKind, Type, Unset, Value};

/// Returns a spec with one shown setting whose type is `ty` and whose unset
/// rule is `unset`.
fn setting(ty: &str, unset: &str) -> String {
    one(&format!(
        "purpose = \"Whether shelf prints each query.\"\ntype = {ty}\nunset = {unset}\n\
         show = \"shown\"\nsource = \"setting\""
    ))
}

/// The type whose table lists `1` and `yes` for true, and `0` and `no` for
/// false.
const SPELLED: &str = "{ boolean = { true = [\"1\", \"yes\"], false = [\"0\", \"no\"] } }";

/// The type of the one variable `SHELF_X` in `text`, which is a valid spec.
fn only_type(text: &str) -> Type {
    spec(text)
        .variable(&name("SHELF_X"))
        .unwrap()
        .value_type()
        .clone()
}

#[test]
fn a_boolean_type_table_reads_both_lists_in_order() {
    let ty = only_type(&setting(SPELLED, "\"required\""));

    let Type::Boolean {
        spellings: Some(spellings),
    } = &ty
    else {
        panic!("{ty:?} has no spellings");
    };
    assert_eq!(spellings.for_true(), ["1", "yes"]);
    assert_eq!(spellings.for_false(), ["0", "no"]);
}

#[test]
fn the_word_boolean_declares_no_spellings() {
    let ty = only_type(&setting("\"boolean\"", "\"required\""));

    assert_eq!(ty, Type::Boolean { spellings: None });
}

#[test]
fn a_spelled_boolean_reads_only_its_spellings_exactly() {
    let ty = only_type(&setting(SPELLED, "\"required\""));

    let read = |text: &str| ty.parse(OsStr::new(text));
    assert_eq!(
        read("yes"),
        Ok(Some(Value::Spelled {
            value: true,
            spelling: "yes".to_owned()
        }))
    );
    assert_eq!(
        read("0"),
        Ok(Some(Value::Spelled {
            value: false,
            spelling: "0".to_owned()
        }))
    );
    for other in ["YES", "true", "on", "", " 1"] {
        let error = read(other).unwrap_err();
        assert_eq!(
            error.to_string(),
            "the value is not one of 1, yes for true and 0, no for false",
            "{other:?}"
        );
    }
}

#[test]
fn a_boolean_type_table_needs_both_lists() {
    for ty in [
        "{ boolean = {} }",
        "{ boolean = { true = [\"1\"] } }",
        "{ boolean = { false = [\"0\"] } }",
    ] {
        let found = kinds(&setting(ty, "\"required\""));

        assert!(
            found
                .iter()
                .all(|kind| *kind == SpecProblemKind::MissingField)
                && !found.is_empty(),
            "{ty}: {found:?}"
        );
    }
}

#[test]
fn an_empty_list_or_a_repeated_spelling_is_an_empty_enumeration() {
    for lists in [
        "true = [], false = [\"0\"]",
        "true = [\"1\"], false = []",
        "true = [\"1\", \"\"], false = [\"0\"]",
        "true = [\"1\", \"1\"], false = [\"0\"]",
        "true = [\"1\", \"on\"], false = [\"0\", \"on\"]",
    ] {
        let ty = format!("{{ boolean = {{ {lists} }} }}");

        assert_eq!(
            kinds(&setting(&ty, "\"required\"")),
            [SpecProblemKind::EmptyEnumeration],
            "{lists}"
        );
    }
}

#[test]
fn a_spelling_in_both_lists_is_refused_with_its_text() {
    let ty = "{ boolean = { true = [\"1\", \"on\"], false = [\"0\", \"on\"] } }";

    let error = Spec::parse(&setting(ty, "\"required\"")).unwrap_err();

    assert_eq!(
        error.problems()[0].to_string(),
        "`variables.SHELF_X.type.boolean` does not fit, because a boolean lists the spelling \
         \"on\" twice"
    );
}

#[test]
fn a_boolean_type_table_refuses_another_field_and_a_spelling_that_is_not_text() {
    let other = "{ boolean = { true = [\"1\"], false = [\"0\"], case = \"any\" } }";
    let number = "{ boolean = { true = [1], false = [\"0\"] } }";

    assert_eq!(
        kinds(&setting(other, "\"required\"")),
        [SpecProblemKind::UnknownField]
    );
    assert_eq!(
        kinds(&setting(number, "\"required\"")),
        [SpecProblemKind::BadValue]
    );
}

#[test]
fn a_default_of_a_spelled_boolean_is_one_of_its_spellings() {
    assert_eq!(
        kinds(&setting(SPELLED, "{ default = \"false\" }")),
        [SpecProblemKind::BadDefault]
    );

    let parsed = spec(&setting(SPELLED, "{ default = \"no\" }"));
    let variable = parsed.variable(&name("SHELF_X")).unwrap();
    assert_eq!(
        variable.unset().in_mode(crate::Mode::Local),
        &Unset::Default(Value::Spelled {
            value: false,
            spelling: "no".to_owned()
        })
    );
}

#[test]
fn the_canonical_form_keeps_the_spellings_and_parses_back_to_the_same_spec() {
    let original = spec(&setting(SPELLED, "{ default = \"no\" }"));

    let canonical = original.to_toml();

    assert!(
        canonical.contains(
            "type = { boolean = { true = [\"1\", \"yes\"], false = [\"0\", \"no\"] } }\n\
             unset = { default = \"no\" }\n"
        ),
        "{canonical}"
    );
    assert_eq!(spec(&canonical), original);
}

#[test]
fn changed_spellings_change_the_digest() {
    let word = spec(&setting("\"boolean\"", "\"required\""));
    let spelled = spec(&setting(SPELLED, "\"required\""));
    let other = spec(&setting(
        "{ boolean = { true = [\"1\"], false = [\"0\", \"no\"] } }",
        "\"required\"",
    ));

    assert_ne!(word.digest(), spelled.digest());
    assert_ne!(spelled.digest(), other.digest());
}
