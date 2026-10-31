//! Tests for the binds a variable's source takes.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::Path;

use proptest::prelude::*;

use super::{
    Bindings, Bound, OnePasswordRef, PathTemplate, PathTemplateError, Placeholder, ProvidedRef,
    ProviderKind, SecretBytes,
};
use crate::Name;

/// The three placeholders, each with the text that names it in a template.
const PLACEHOLDERS: [(Placeholder, &str); 3] = [
    (Placeholder::Workspace, "{workspace}"),
    (Placeholder::Checkout, "{checkout}"),
    (Placeholder::Home, "{home}"),
];

/// Returns the qualifiers of `pairs`.
fn qualifiers(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

// ProviderKind

#[test]
fn a_provider_kind_takes_a_host_and_a_kind_of_lower_case_words() {
    for text in ["multiverse/port", "host-2/data-dir"] {
        let kind: ProviderKind = text.parse().unwrap();

        assert_eq!(kind.as_str(), text);
        assert_eq!(kind.to_string(), text);
    }
}

#[test]
fn a_provider_kind_refuses_any_other_text() {
    for text in [
        "multiverse",
        "multiverse/port/api",
        "/port",
        "multiverse/",
        "",
        "Multiverse/port",
        "multiverse/2port",
        "multi verse/port",
    ] {
        assert!(
            text.parse::<ProviderKind>().is_err(),
            "{text:?} was accepted"
        );
    }
}

// ProvidedRef

#[test]
fn a_provided_ref_holds_its_kind_and_lists_its_qualifiers_in_name_order() {
    let kind: ProviderKind = "multiverse/url".parse().unwrap();
    let reference =
        ProvidedRef::new(kind, qualifiers(&[("process", "api"), ("app", "shelf")])).unwrap();

    assert_eq!(reference.kind().as_str(), "multiverse/url");
    assert_eq!(reference.qualifier("process"), Some("api"));
    assert_eq!(reference.qualifier("theme"), None);
    assert_eq!(
        reference.qualifiers().collect::<Vec<_>>(),
        [("app", "shelf"), ("process", "api")]
    );
}

#[test]
fn a_provided_ref_refuses_a_qualifier_named_after_a_bind_kind_or_not_lower_case() {
    for key in ["provided", "path", "one-password", "App", "2app"] {
        let kind: ProviderKind = "multiverse/url".parse().unwrap();

        assert!(
            ProvidedRef::new(kind, qualifiers(&[(key, "x")])).is_err(),
            "{key:?} was accepted"
        );
    }
}

// PathTemplate

#[test]
fn a_path_template_lists_its_placeholders_in_order() {
    let template: PathTemplate = "{home}/{workspace}/books.db".parse().unwrap();

    assert_eq!(
        template.placeholders().collect::<Vec<_>>(),
        [Placeholder::Home, Placeholder::Workspace]
    );
    assert_eq!(template.as_str(), "{home}/{workspace}/books.db");
}

#[test]
fn a_path_template_without_placeholders_is_a_plain_path() {
    let template: PathTemplate = "/var/lib/shelf".parse().unwrap();

    assert_eq!(template.placeholders().count(), 0);
}

#[test]
fn a_path_template_fills_each_placeholder_from_its_place() {
    let template: PathTemplate = "{home}/.cache/{workspace}/books.db".parse().unwrap();
    let path = template.fill(|placeholder| match placeholder {
        Placeholder::Home => Some(Path::new("/home/luke")),
        Placeholder::Workspace => Some(Path::new("/workspace")),
        _ => None,
    });

    // PathBuf equality ignores a doubled slash, so the text is compared.
    assert_eq!(
        path.as_ref().map(|path| path.as_os_str()),
        Ok(OsStr::new("/home/luke/.cache//workspace/books.db"))
    );
}

#[test]
fn a_path_template_fills_to_the_first_placeholder_that_has_no_place() {
    let template: PathTemplate = "{checkout}/{home}/{workspace}".parse().unwrap();
    let path = template.fill(|placeholder| {
        (placeholder == Placeholder::Checkout).then_some(Path::new("/checkout"))
    });

    assert_eq!(path, Err(Placeholder::Home));
}

#[test]
fn a_path_template_refuses_an_unknown_placeholder() {
    assert_eq!(
        "{workspace}/{Workspace}/books.db".parse::<PathTemplate>(),
        Err(PathTemplateError::UnknownPlaceholder {
            name: "Workspace".to_owned()
        })
    );
}

#[test]
fn a_path_template_refuses_a_brace_outside_a_placeholder() {
    for text in [
        "{workspace/books.db",
        "books}.db",
        "{{workspace}}",
        "{home}}",
    ] {
        assert_eq!(
            text.parse::<PathTemplate>(),
            Err(PathTemplateError::Brace),
            "{text:?}"
        );
    }
}

#[test]
fn a_path_template_refuses_empty_text() {
    assert_eq!("".parse::<PathTemplate>(), Err(PathTemplateError::Empty));
}

#[test]
fn a_placeholder_prints_as_its_braced_name() {
    for (placeholder, text) in PLACEHOLDERS {
        assert_eq!(placeholder.to_string(), text);
    }
}

/// Text without braces, which a template may hold between placeholders.
fn plain() -> impl Strategy<Value = String> {
    "[a-z0-9/._ -]{0,8}"
}

proptest! {
    #[test]
    fn a_template_of_known_placeholders_and_plain_text_parses_and_lists_them(
        parts in prop::collection::vec((plain(), 0..3_usize), 1..5),
        tail in plain(),
    ) {
        let mut text = String::new();
        let mut expected = Vec::new();
        for (before, index) in &parts {
            let (placeholder, name) = PLACEHOLDERS[*index];
            text.push_str(before);
            text.push_str(name);
            expected.push(placeholder);
        }
        text.push_str(&tail);

        let template: PathTemplate = text.parse().unwrap();

        prop_assert_eq!(template.placeholders().collect::<Vec<_>>(), expected);
        prop_assert_eq!(template.as_str(), text);
    }

    #[test]
    fn a_template_that_names_any_other_placeholder_is_refused(
        before in plain(),
        name in "[a-z]{1,10}",
        after in plain(),
    ) {
        prop_assume!(!["workspace", "checkout", "home"].contains(&name.as_str()));
        let text = format!("{before}{{{name}}}{after}");

        prop_assert_eq!(
            text.parse::<PathTemplate>(),
            Err(PathTemplateError::UnknownPlaceholder { name })
        );
    }

    #[test]
    fn a_template_with_a_lone_brace_is_refused(
        before in plain(),
        brace in prop::sample::select(vec!['{', '}']),
        after in plain(),
    ) {
        let text = format!("{before}{brace}{after}");

        prop_assert_eq!(text.parse::<PathTemplate>(), Err(PathTemplateError::Brace));
    }
}

// OnePasswordRef

#[test]
fn a_one_password_ref_takes_three_or_four_parts() {
    for text in [
        "op://Personal/shelf mirror/token",
        "op://Personal/shelf/api/token",
    ] {
        let reference: OnePasswordRef = text.parse().unwrap();

        assert_eq!(reference.as_str(), text);
        assert_eq!(reference.to_string(), text);
    }
}

#[test]
fn a_one_password_ref_refuses_any_other_text() {
    for text in [
        "https://Personal/shelf/token",
        "op://Personal/token",
        "op://a/b/c/d/e",
        "op://Personal//token",
        "op://Personal/shelf\n/token",
    ] {
        assert!(
            text.parse::<OnePasswordRef>().is_err(),
            "{text:?} was accepted"
        );
    }
}

// SecretBytes

#[test]
fn secret_bytes_default_to_32() {
    assert_eq!(SecretBytes::default().get(), 32);
    assert_eq!(SecretBytes::DEFAULT, SecretBytes::default());
}

proptest! {
    #[test]
    fn secret_bytes_accept_a_number_exactly_when_it_lies_from_16_to_1024(bytes in any::<u16>()) {
        prop_assert_eq!(
            SecretBytes::new(bytes).map(SecretBytes::get).ok(),
            (16..=1024).contains(&bytes).then_some(bytes)
        );
    }
}

// Bindings

/// Returns the name `text`.
fn name(text: &str) -> Name {
    text.parse().unwrap()
}

#[test]
fn binding_a_name_again_replaces_its_value() {
    let mut bindings = Bindings::new();
    bindings.bind(name("SHELF_PORT"), Bound::Plain("8080".into()));
    bindings.bind(name("SHELF_PORT"), Bound::NotUnicode);

    assert!(matches!(
        bindings.get(&name("SHELF_PORT")),
        Some(Bound::NotUnicode)
    ));
}
