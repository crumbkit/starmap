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

#[test]
fn an_added_check_sentence_is_a_change_of_the_check() {
    let with = TOKEN.replace(
        "purpose = \"The token.\"\n",
        "purpose = \"The token.\"\ncheck = \"The token starts with tk_.\"\n",
    );

    assert_eq!(
        changes(Some(&spec(TOKEN)), &spec(&with)),
        vec![Change {
            change: Kind::Changed,
            name: "SHELF_TOKEN".to_owned(),
            field: Some("check".to_owned()),
            was: None,
            now: Some("The token starts with tk_.".to_owned()),
        }]
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

#[test]
fn reading_a_boolean_by_its_presence_is_a_change_of_the_type() {
    let plain = LISTEN
        .replace("\"port\"", "\"boolean\"")
        .replace("\"required\"", "\"optional\"");
    let presence = plain.replace("\"boolean\"", "\"boolean\"\npresence = true");

    assert_eq!(
        changes(Some(&spec(&plain)), &spec(&presence)),
        vec![Change {
            change: Kind::Changed,
            name: "SHELF_LISTEN".to_owned(),
            field: Some("type".to_owned()),
            was: Some("boolean".to_owned()),
            now: Some("boolean (true when set to any value)".to_owned()),
        }]
    );
}

/// A log level setting whose type is the named enumeration `log-level`.
const LOG_LEVEL: &str = r#"
[enumerations.log-level]
debug = "Logs every request."
info = "Logs each request once."

[variables.SHELF_LOG_LEVEL]
purpose = "How much the api process logs."
type = { enumeration = "log-level" }
unset = "required"
show = "shown"
source = "setting"
"#;

/// Returns a change of `field` of `SHELF_LOG_LEVEL` from `was` to `now`.
fn log_level_change(field: &str, was: &str, now: &str) -> Change {
    Change {
        change: Kind::Changed,
        name: "SHELF_LOG_LEVEL".to_owned(),
        field: Some(field.to_owned()),
        was: Some(was.to_owned()),
        now: Some(now.to_owned()),
    }
}

#[test]
fn a_changed_description_is_a_change_of_the_values() {
    let now = LOG_LEVEL.replace("Logs every request.", "Logs every request and its timing.");

    assert_eq!(
        changes(Some(&spec(LOG_LEVEL)), &spec(&now)),
        vec![log_level_change(
            "values",
            "debug: Logs every request.; info: Logs each request once.",
            "debug: Logs every request and its timing.; info: Logs each request once.",
        )]
    );
}

#[test]
fn a_change_from_an_inline_list_to_a_named_enumeration_is_a_change_of_the_type() {
    let inline = LOG_LEVEL.replace(
        "{ enumeration = \"log-level\" }",
        "{ enumeration = [\"debug\", \"info\"] }",
    );

    let fields: Vec<Option<String>> = changes(Some(&spec(&inline)), &spec(LOG_LEVEL))
        .into_iter()
        .map(|change| change.field)
        .collect();

    assert_eq!(
        fields,
        vec![Some("type".to_owned()), Some("values".to_owned())]
    );
}

#[test]
fn a_renamed_enumeration_is_a_change_of_the_type_alone() {
    let renamed = LOG_LEVEL.replace("log-level", "verbosity");

    assert_eq!(
        changes(Some(&spec(LOG_LEVEL)), &spec(&renamed)),
        vec![log_level_change(
            "type",
            "log-level: one of debug, info",
            "verbosity: one of debug, info",
        )]
    );
}

/// Two optional settings, which `group` may put in a group.
fn mirror(group: &str) -> String {
    format!(
        "{group}\n\
         [variables.SHELF_MIRROR_ACCOUNT]\npurpose = \"The account.\"\ntype = \"string\"\n\
         unset = \"optional\"\nshow = \"shown\"\nsource = \"setting\"\n\n\
         [variables.SHELF_MIRROR_KEY]\npurpose = \"The key.\"\ntype = \"string\"\n\
         unset = \"optional\"\nshow = \"shown\"\nsource = \"setting\"\n"
    )
}

/// The group `name` of both settings.
fn group(name: &str) -> String {
    format!("[groups.{name}]\nvariables = [\"SHELF_MIRROR_ACCOUNT\", \"SHELF_MIRROR_KEY\"]\n")
}

/// Returns a change of the group of `variable` from `was` to `now`.
fn group_change(variable: &str, was: Option<&str>, now: Option<&str>) -> Change {
    Change {
        change: Kind::Changed,
        name: variable.to_owned(),
        field: Some("group".to_owned()),
        was: was.map(str::to_owned),
        now: now.map(str::to_owned),
    }
}

#[test]
fn a_variable_that_joins_a_group_is_a_change_of_the_group() {
    assert_eq!(
        changes(Some(&spec(&mirror(""))), &spec(&mirror(&group("mirror")))),
        vec![
            group_change("SHELF_MIRROR_ACCOUNT", None, Some("mirror")),
            group_change("SHELF_MIRROR_KEY", None, Some("mirror")),
        ]
    );
}

#[test]
fn a_renamed_group_is_a_change_of_the_group_of_each_member() {
    assert_eq!(
        changes(
            Some(&spec(&mirror(&group("mirror")))),
            &spec(&mirror(&group("upstream")))
        ),
        vec![
            group_change("SHELF_MIRROR_ACCOUNT", Some("mirror"), Some("upstream")),
            group_change("SHELF_MIRROR_KEY", Some("mirror"), Some("upstream")),
        ]
    );
}
