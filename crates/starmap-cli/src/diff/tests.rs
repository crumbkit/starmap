use starmap::Spec;

use super::*;

/// Returns the spec that `text` holds.
fn spec(text: &str) -> Spec {
    Spec::parse(text).unwrap()
}

/// A withheld secret with a 1Password bind.
const TOKEN: &str = r#"
[variables.SHELF_TOKEN]
purpose = "The token."
type = "string"
unset = "required"
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

/// A port setting, which a later spec turns into a socket address.
const LISTEN: &str = r#"
[variables.SHELF_LISTEN]
purpose = "Where the api process listens."
type = "port"
unset = "required"
show = "shown"
source = "setting"
"#;

#[test]
fn a_changed_pattern_is_a_change_of_the_type() {
    let four = LISTEN.replace("\"port\"", "{ string = { pattern = \"[0-9]{4}\" } }");
    let five = LISTEN.replace("\"port\"", "{ string = { pattern = \"[0-9]{5}\" } }");

    assert_eq!(
        changes(Some(&spec(&four)), &spec(&five)),
        vec![Change {
            change: Kind::Changed,
            name: "SHELF_LISTEN".to_owned(),
            field: Some("type".to_owned()),
            was: Some("string matching [0-9]{4}".to_owned()),
            now: Some("string matching [0-9]{5}".to_owned()),
        }]
    );
}

#[test]
fn a_socket_address_is_named_when_it_is_added_and_when_a_type_changes_to_it() {
    let socket = LISTEN.replace("\"port\"", "\"socket-address\"");

    let added = changes(None, &spec(&socket));
    assert_eq!(
        added
            .first()
            .and_then(|change| change.now.clone())
            .as_deref(),
        Some("ip socket address, shown, setting")
    );

    let changed = changes(Some(&spec(LISTEN)), &spec(&socket));
    assert_eq!(
        changed,
        vec![Change {
            change: Kind::Changed,
            name: "SHELF_LISTEN".to_owned(),
            field: Some("type".to_owned()),
            was: Some("port".to_owned()),
            now: Some("ip socket address".to_owned()),
        }]
    );
}

#[test]
fn turning_a_cache_off_is_a_change_of_the_cache_and_not_of_the_bind() {
    let now = TOKEN.replace(
        r#"bind = { one-password = "op://Personal/shelf/token" }"#,
        r#"bind = { one-password = "op://Personal/shelf/token", cache = false }"#,
    );
    let changes = changes(Some(&spec(TOKEN)), &spec(&now));
    assert_eq!(
        changes,
        vec![Change {
            change: Kind::Changed,
            name: "SHELF_TOKEN".to_owned(),
            field: Some("cache".to_owned()),
            was: Some("allowed".to_owned()),
            now: Some("never".to_owned()),
        }]
    );
}

#[test]
fn a_new_reference_with_cache_off_is_a_change_of_the_bind_and_the_cache() {
    let now = TOKEN.replace(
        r#"bind = { one-password = "op://Personal/shelf/token" }"#,
        r#"bind = { one-password = "op://Personal/shelf/key", cache = false }"#,
    );
    let fields: Vec<_> = changes(Some(&spec(TOKEN)), &spec(&now))
        .into_iter()
        .map(|change| change.field)
        .collect();
    assert_eq!(fields, [Some("bind".to_owned()), Some("cache".to_owned())]);
}

#[test]
fn writing_cache_true_is_no_change() {
    let now = TOKEN.replace(
        r#"bind = { one-password = "op://Personal/shelf/token" }"#,
        r#"bind = { one-password = "op://Personal/shelf/token", cache = true }"#,
    );
    assert_eq!(changes(Some(&spec(TOKEN)), &spec(&now)), Vec::new());
}
