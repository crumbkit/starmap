//! Tests for the `[families]` table: its parsing, its refusals, its
//! canonical form and its digest.

use crate::{
    Declaration, Mode, Name, Pointer, Source, Spec, SpecProblemKind, Type, Unset, capture,
};

/// A spec with the open family `SHELF_DB_URL` and the listed family
/// `SHELF_TOKEN`, beside one declared variable.
const FAMILIES: &str = r#"
[families.SHELF_DB_URL]
purpose = "The database of one region, keyed by the region's code."
type = "url"
unset = "optional"
show = "shown"
source = "setting"

[families.SHELF_TOKEN]
purpose = "The API token of one tenant."
type = "string"
show = "withheld"
source = "setting"
keys = ["GLOBEX", "ACME"]
unset = "required"

[variables.SHELF_PORT]
purpose = "The port the api process listens on."
type = "port"
unset = { default = "8080" }
show = "shown"
source = "setting"
"#;

/// An open family's table without its `unset`, `source` and `keys`.
const OPEN: &str = "[families.SHELF_DB_URL]\npurpose = \"The database of one region.\"\n\
                    type = \"url\"\nshow = \"shown\"\n";

/// Returns the name `text`.
fn name(text: &str) -> Name {
    text.parse().unwrap()
}

/// Parses `text`, which is not a valid spec, and returns the kind and the
/// message of each problem.
fn problems(text: &str) -> Vec<(SpecProblemKind, String)> {
    Spec::parse(text)
        .unwrap_err()
        .problems()
        .iter()
        .map(|problem| (problem.kind(), problem.to_string()))
        .collect()
}

/// Returns the kind of each problem of `text`.
fn kinds(text: &str) -> Vec<SpecProblemKind> {
    problems(text).into_iter().map(|(kind, _)| kind).collect()
}

#[test]
fn an_open_family_parses_with_the_shared_fields_and_no_keys() {
    let spec = Spec::parse(FAMILIES).unwrap();

    let family = spec.family(&name("SHELF_DB_URL")).unwrap();

    assert_eq!(family.prefix().as_str(), "SHELF_DB_URL");
    assert_eq!(family.keys(), None);
    assert_eq!(family.variable().value_type(), &Type::Url { schemes: None });
    assert_eq!(family.variable().source(), &Source::Setting { bind: None });
    assert_eq!(family.key_of(&name("SHELF_DB_URL_EU")), Some("EU"));
}

#[test]
fn a_listed_family_parses_and_its_keys_come_back_in_key_order() {
    let spec = Spec::parse(FAMILIES).unwrap();

    let family = spec.family(&name("SHELF_TOKEN")).unwrap();

    assert_eq!(
        family.keys(),
        Some(["ACME".to_owned(), "GLOBEX".to_owned()].as_slice())
    );
    assert_eq!(
        family.variable().unset().in_mode(Mode::Local),
        &Unset::Required
    );
}

#[test]
fn families_are_listed_in_prefix_order() {
    let spec = Spec::parse(FAMILIES).unwrap();

    let prefixes: Vec<&str> = spec
        .families()
        .map(|family| family.prefix().as_str())
        .collect();

    assert_eq!(prefixes, ["SHELF_DB_URL", "SHELF_TOKEN"]);
}

#[test]
fn a_member_name_is_declared_by_its_family_and_a_variable_by_itself() {
    let spec = Spec::parse(FAMILIES).unwrap();

    let eu = name("SHELF_DB_URL_EU");
    let unlisted = name("SHELF_TOKEN_INITECH");
    let port = name("SHELF_PORT");

    assert!(matches!(
        spec.declared(&eu),
        Some(Declaration::Member { key: "EU", .. })
    ));
    assert!(matches!(
        spec.declared(&unlisted),
        Some(Declaration::Member { key: "INITECH", .. })
    ));
    assert!(matches!(
        spec.declared(&port),
        Some(Declaration::Variable(_))
    ));
    assert_eq!(spec.declared(&name("SHELF_DB_URL")), None);
}

#[test]
fn a_key_with_a_lower_case_letter_is_not_a_member() {
    let spec = Spec::parse(FAMILIES).unwrap();

    let family = spec.family(&name("SHELF_DB_URL")).unwrap();
    let vars = [("SHELF_DB_URL_Eu", "https://eu.example/")]
        .map(|(name, value)| (name.into(), value.into()));

    assert_eq!(capture(&spec, vars).iter().count(), 0);
    assert_eq!(family.key_of(&name("SHELF_DB_URL__EU")), None);
    assert_eq!(family.key_of(&name("SHELF_DB_URLEU")), None);
    assert_eq!(family.key_of(&name("SHELF_DB_URL_")), None);
}

#[test]
fn a_key_that_breaks_the_key_rule_fails_with_bad_value() {
    let text = FAMILIES.replace("[\"GLOBEX\", \"ACME\"]", "[\"GLOBEX\", \"acme\"]");

    let found = problems(&text);

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, SpecProblemKind::BadValue);
    assert!(
        found[0]
            .1
            .contains("`families.SHELF_TOKEN.keys[1]` is \"acme\""),
        "{found:?}"
    );
}

#[test]
fn an_empty_key_list_fails_with_bad_family() {
    let text = FAMILIES.replace("[\"GLOBEX\", \"ACME\"]", "[]");

    assert_eq!(kinds(&text), [SpecProblemKind::BadFamily]);
}

#[test]
fn a_repeated_key_fails_with_bad_family() {
    let text = FAMILIES.replace("[\"GLOBEX\", \"ACME\"]", "[\"ACME\", \"GLOBEX\", \"ACME\"]");

    let found = problems(&text);

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, SpecProblemKind::BadFamily);
    assert!(found[0].1.contains("\"ACME\" twice"), "{found:?}");
}

#[test]
fn a_key_that_makes_the_member_name_too_long_fails_with_bad_family() {
    let long = "K".repeat(Name::MAX_BYTES - "SHELF_TOKEN_".len() + 1);
    let text = FAMILIES.replace("\"ACME\"]", &format!("\"{long}\"]"));

    let found = problems(&text);

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, SpecProblemKind::BadFamily);
    assert!(found[0].1.contains("129 bytes long"), "{found:?}");
}

#[test]
fn a_default_fails_with_bad_family_for_an_open_and_a_listed_family() {
    let open =
        format!("{OPEN}unset = {{ default = \"https://eu.example\" }}\nsource = \"setting\"\n");
    let listed = FAMILIES
        .replace(
            "keys = [\"GLOBEX\", \"ACME\"]\nunset = \"required\"",
            "keys = [\"GLOBEX\", \"ACME\"]\nunset = { default = \"x\" }",
        )
        .replace("show = \"withheld\"", "show = \"shown\"");

    assert_eq!(kinds(&open), [SpecProblemKind::BadFamily]);
    assert_eq!(kinds(&listed), [SpecProblemKind::BadFamily]);
}

#[test]
fn a_required_open_family_fails_with_bad_family() {
    let required = format!("{OPEN}unset = \"required\"\nsource = \"setting\"\n");
    let in_production = format!(
        "{OPEN}unset = {{ local = \"optional\", production = \"required\" }}\nsource = \"setting\"\n"
    );

    let found = problems(&required);

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, SpecProblemKind::BadFamily);
    assert!(
        found[0]
            .1
            .contains("only a family with `keys` may be required"),
        "{found:?}"
    );
    assert_eq!(kinds(&in_production), [SpecProblemKind::BadFamily]);
}

#[test]
fn a_listed_family_may_be_required_in_production_and_optional_locally() {
    let text = FAMILIES.replace(
        "unset = \"required\"",
        "unset = { local = \"optional\", production = \"required\" }",
    );

    let spec = Spec::parse(&text).unwrap();
    let family = spec.family(&name("SHELF_TOKEN")).unwrap();

    assert_eq!(
        family.variable().unset().in_mode(Mode::Local),
        &Unset::Optional
    );
    assert_eq!(
        family.variable().unset().in_mode(Mode::Production),
        &Unset::Required
    );
}

#[test]
fn each_source_that_needs_a_bind_per_member_fails_with_bad_family() {
    let unset = "unset = \"optional\"\n";
    let secret = OPEN
        .replace("show = \"shown\"", "show = \"withheld\"")
        .replace("type = \"url\"", "type = \"string\"")
        + unset
        + "source = \"secret\"\n";
    let generated = OPEN
        .replace("show = \"shown\"", "show = \"withheld\"")
        .replace("type = \"url\"", "type = \"string\"")
        + unset
        + "source = \"generated\"\n";
    let provided =
        format!("{OPEN}{unset}source = \"provided\"\nbind = {{ provided = \"host/db\" }}\n");
    let bound = format!("{OPEN}{unset}source = \"setting\"\nbind = {{ path = \"{{home}}/db\" }}\n")
        .replace("type = \"url\"", "type = \"path\"");

    for text in [secret, generated, provided, bound] {
        assert_eq!(kinds(&text), [SpecProblemKind::BadFamily], "{text}");
    }
}

#[test]
fn an_inherited_withheld_family_parses() {
    let text = OPEN
        .replace("show = \"shown\"", "show = \"withheld\"")
        .replace("type = \"url\"", "type = \"string\"")
        + "unset = \"optional\"\nsource = \"inherited\"\n";

    let spec = Spec::parse(&text).unwrap();

    assert_eq!(spec.families().count(), 1);
}

#[test]
fn a_variable_that_is_a_member_fails_with_bad_family() {
    let text = format!(
        "{FAMILIES}\n[variables.SHELF_DB_URL_EU]\npurpose = \"The European database.\"\n\
         type = \"url\"\nunset = \"optional\"\nshow = \"shown\"\nsource = \"setting\"\n"
    );

    let found = problems(&text);

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, SpecProblemKind::BadFamily);
    assert!(
        found[0]
            .1
            .contains("`variables.SHELF_DB_URL_EU` is a member of the family SHELF_DB_URL"),
        "{found:?}"
    );
}

#[test]
fn a_mode_variable_that_is_a_member_fails_with_bad_family() {
    let text = format!(
        "[mode]\nvariable = \"SHELF_DB_URL_ENV\"\nproduction = [\"prod\"]\n{FAMILIES}\n\
         [variables.SHELF_DB_URL_ENV]\npurpose = \"The stage.\"\ntype = \"string\"\n\
         unset = \"optional\"\nshow = \"shown\"\nsource = \"setting\"\n"
    );

    assert!(kinds(&text).contains(&SpecProblemKind::BadFamily));
}

#[test]
fn overlapping_prefixes_fail_with_bad_family() {
    let text = format!(
        "{FAMILIES}\n[families.SHELF_DB]\npurpose = \"A database.\"\ntype = \"url\"\n\
         unset = \"optional\"\nshow = \"shown\"\nsource = \"setting\"\n"
    );

    let found = problems(&text);

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, SpecProblemKind::BadFamily);
    assert!(
        found[0]
            .1
            .contains("`families.SHELF_DB_URL` is a member name of the family SHELF_DB"),
        "{found:?}"
    );
}

#[test]
fn a_prefix_that_is_not_a_variable_name_fails_with_bad_name() {
    let text = FAMILIES.replace("[families.SHELF_DB_URL]", "[families.shelf_db_url]");

    assert_eq!(kinds(&text), [SpecProblemKind::BadName]);
}

#[test]
fn a_family_refuses_a_field_it_does_not_take() {
    let text = FAMILIES.replace("keys = [", "members = [");

    assert!(kinds(&text).contains(&SpecProblemKind::UnknownField));
}

#[test]
fn a_pointer_that_declares_a_family_fails_with_pointer_declares() {
    let text = format!("spec = \"other.toml\"\n{FAMILIES}");

    let error = Pointer::parse(&text).unwrap_err();

    assert!(
        error
            .problems()
            .iter()
            .any(|problem| problem.kind() == SpecProblemKind::PointerDeclares)
    );
}

#[test]
fn the_canonical_form_of_families_round_trips() {
    let spec = Spec::parse(FAMILIES).unwrap();

    let canonical = spec.to_toml();
    let again = Spec::parse(&canonical).unwrap();

    assert_eq!(again, spec);
    assert!(canonical.contains("[families.SHELF_TOKEN]"), "{canonical}");
    assert!(
        canonical.contains("keys = [\"ACME\", \"GLOBEX\"]"),
        "{canonical}"
    );
    let families = canonical.find("[families.").unwrap();
    let variables = canonical.find("[variables.").unwrap();
    assert!(families < variables, "{canonical}");
}

#[test]
fn adding_a_family_changes_the_digest() {
    let (families, variable) = FAMILIES.split_at(FAMILIES.find("[variables.").unwrap());
    let without = Spec::parse(variable).unwrap();
    let with = Spec::parse(&format!("{families}{variable}")).unwrap();

    assert_ne!(without.digest(), with.digest());
    assert!(!without.to_toml().contains("families"));
}

#[test]
fn adding_a_listed_key_changes_the_digest_and_reordering_the_keys_does_not() {
    let spec = Spec::parse(FAMILIES).unwrap();
    let reordered =
        Spec::parse(&FAMILIES.replace("[\"GLOBEX\", \"ACME\"]", "[\"ACME\", \"GLOBEX\"]")).unwrap();
    let added = Spec::parse(&FAMILIES.replace(
        "[\"GLOBEX\", \"ACME\"]",
        "[\"GLOBEX\", \"ACME\", \"INITECH\"]",
    ))
    .unwrap();

    assert_eq!(spec.digest(), reordered.digest());
    assert_ne!(spec.digest(), added.digest());
}
