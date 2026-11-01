use starmap::Spec;

use super::*;

/// Returns the spec that `text` holds.
fn spec(text: &str) -> Spec {
    Spec::parse(text).unwrap()
}

/// A withheld secret with a default and a 1Password bind.
const TOKEN: &str = r#"
[variables.SHELF_TOKEN]
purpose = "The token."
type = "string"
unset = { default = "a-default-secret" }
show = "withheld"
source = "secret"
bind = { one-password = "op://Personal/shelf/token" }
"#;

#[test]
fn equal_specs_have_no_changes() {
    assert_eq!(changes(Some(&spec(TOKEN)), &spec(TOKEN)), Vec::new());
}

#[test]
fn a_spec_against_no_spec_counts_every_variable_as_added() {
    let changes = changes(None, &spec(TOKEN));
    assert_eq!(
        changes,
        vec![Change {
            change: Kind::Added,
            name: "SHELF_TOKEN".to_owned(),
            field: None,
            was: None,
            now: Some("string, withheld, secret".to_owned()),
        }]
    );
}

#[test]
fn a_changed_bind_and_purpose_are_one_change_each_in_field_order() {
    let now = TOKEN
        .replace("The token.", "The mirror's token.")
        .replace("shelf/token", "shelf/mirror");
    let fields: Vec<Option<String>> = changes(Some(&spec(TOKEN)), &spec(&now))
        .into_iter()
        .map(|change| change.field)
        .collect();
    assert_eq!(
        fields,
        vec![Some("bind".to_owned()), Some("purpose".to_owned())]
    );
}

#[test]
fn a_changed_withheld_default_is_listed_and_prints_as_dots_and_never_as_the_secret() {
    let now = TOKEN.replace("a-default-secret", "another-secret");
    let changes = changes(Some(&spec(TOKEN)), &spec(&now));
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].field.as_deref(), Some("unset"));
    let text = format!("{changes:?}");
    assert!(!text.contains("a-default-secret"), "{text}");
    assert!(!text.contains("another-secret"), "{text}");
}
