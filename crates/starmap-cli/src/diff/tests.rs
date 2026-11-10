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

#[test]
fn an_added_changed_or_removed_effect_is_a_change_of_the_effect() {
    let with = |effect: &str| {
        TOKEN.replace(
            "purpose = \"The token.\"\n",
            &format!("purpose = \"The token.\"\neffect = \"{effect}\"\n"),
        )
    };
    let change = |was: Option<&str>, now: Option<&str>| Change {
        change: Kind::Changed,
        name: "SHELF_TOKEN".to_owned(),
        field: Some("effect".to_owned()),
        was: was.map(str::to_owned),
        now: now.map(str::to_owned),
    };
    let (old, new) = (with("Uploads fail."), with("Uploads are skipped."));

    assert_eq!(
        changes(Some(&spec(TOKEN)), &spec(&old)),
        vec![change(None, Some("Uploads fail."))]
    );
    assert_eq!(
        changes(Some(&spec(&old)), &spec(&new)),
        vec![change(Some("Uploads fail."), Some("Uploads are skipped."))]
    );
    assert_eq!(
        changes(Some(&spec(&old)), &spec(TOKEN)),
        vec![change(Some("Uploads fail."), None)]
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

/// A withheld key whose type is 32 bytes as base64.
const KEY: &str = r#"
[variables.SHELF_KEY]
purpose = "The key."
type = { bytes = { encoding = "base64", length = 32 } }
unset = "required"
show = "withheld"
source = "secret"
"#;

#[test]
fn an_added_bytes_variable_names_the_type_word_bytes() {
    assert_eq!(
        changes(None, &spec(KEY))
            .into_iter()
            .map(|change| change.now)
            .collect::<Vec<_>>(),
        [Some("bytes, withheld, secret".to_owned())]
    );
}

#[test]
fn a_changed_encoding_or_length_is_a_change_of_the_type() {
    let hex = KEY.replace("\"base64\"", "\"hex\"");
    let longer = KEY.replace("length = 32", "min-length = 64");

    for (now, text) in [
        (hex, "32 bytes as hex"),
        (longer, "at least 64 bytes as base64"),
    ] {
        assert_eq!(
            changes(Some(&spec(KEY)), &spec(&now)),
            vec![Change {
                change: Kind::Changed,
                name: "SHELF_KEY".to_owned(),
                field: Some("type".to_owned()),
                was: Some("32 bytes as base64".to_owned()),
                now: Some(text.to_owned()),
            }]
        );
    }
}

#[test]
fn a_changed_scheme_list_is_a_change_of_the_type() {
    let https = LISTEN.replace("\"port\"", "{ url = { schemes = [\"https\"] } }");
    let both = LISTEN.replace("\"port\"", "{ url = { schemes = [\"https\", \"http\"] } }");

    assert_eq!(
        changes(Some(&spec(&https)), &spec(&both)),
        vec![Change {
            change: Kind::Changed,
            name: "SHELF_LISTEN".to_owned(),
            field: Some("type".to_owned()),
            was: Some("url (https)".to_owned()),
            now: Some("url (https, http)".to_owned()),
        }]
    );
}
