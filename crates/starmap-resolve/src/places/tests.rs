//! Tests for the places a path template names and for expanding a
//! template.

use std::ffi::OsStr;
use std::path::PathBuf;

use proptest::prelude::*;
use starmap::{PathTemplate, Placeholder};

use super::{AppName, Places, WorkspaceName, expand};

/// Returns the template `text`.
fn template(text: &str) -> PathTemplate {
    text.parse().unwrap()
}

/// Returns places with the checkout `/checkout` and every other place.
fn every_place() -> Places {
    Places::new(PathBuf::from("/checkout"))
        .workspace(
            "shelf-dev".parse().unwrap(),
            "shelf".parse().unwrap(),
            PathBuf::from("/workspace"),
        )
        .home(PathBuf::from("/home/luke"))
}

#[test]
fn expand_replaces_each_placeholder_with_its_place() {
    let path = expand(&template("{workspace}/books.db"), &every_place());
    assert_eq!(path, Ok(PathBuf::from("/workspace/books.db")));

    let path = expand(&template("{home}/.cache/{checkout}"), &every_place());
    // PathBuf equality ignores a doubled slash, so the text is compared.
    assert_eq!(
        path.as_ref().map(|path| path.as_os_str()),
        Ok(OsStr::new("/home/luke/.cache//checkout"))
    );
}

#[test]
fn expand_returns_a_template_without_placeholders_unchanged() {
    let path = expand(&template("books.db"), &Places::new(PathBuf::from("/c")));
    assert_eq!(path, Ok(PathBuf::from("books.db")));
}

#[test]
fn expand_names_the_first_placeholder_that_has_no_place() {
    let places = Places::new(PathBuf::from("/checkout"));
    let path = expand(&template("{checkout}/{home}/{workspace}"), &places);
    assert_eq!(path, Err(Placeholder::Home));
}

#[test]
fn a_workspace_or_app_name_holds_lower_case_letters_digits_and_hyphens_and_never_a_slash() {
    for good in ["shelf", "shelf-dev", "a1"] {
        assert!(good.parse::<WorkspaceName>().is_ok(), "{good}");
        assert!(good.parse::<AppName>().is_ok(), "{good}");
    }
    for bad in ["", "Shelf", "1shelf", "shelf/dev", "shelf.dev", "-shelf"] {
        assert!(bad.parse::<WorkspaceName>().is_err(), "{bad:?}");
        assert!(bad.parse::<AppName>().is_err(), "{bad:?}");
    }
    let name: WorkspaceName = "shelf-dev".parse().unwrap();
    assert_eq!(name.as_str(), "shelf-dev");
}

/// One piece of a generated template: literal text or a placeholder.
#[derive(Debug, Clone)]
enum Piece {
    Text(String),
    Place(Placeholder),
}

/// Generates a piece of a template.
fn piece() -> impl Strategy<Value = Piece> {
    prop_oneof![
        "[a-z./_-]{1,6}".prop_map(Piece::Text),
        prop_oneof![
            Just(Placeholder::Workspace),
            Just(Placeholder::Checkout),
            Just(Placeholder::Home),
        ]
        .prop_map(Piece::Place),
    ]
}

/// Returns the directory each test gives `placeholder` when it has a place.
fn dir(placeholder: Placeholder) -> &'static str {
    match placeholder {
        Placeholder::Workspace => "/w",
        Placeholder::Checkout => "/c",
        _ => "/h",
    }
}

proptest! {
    #[test]
    fn expand_succeeds_exactly_when_every_placeholder_it_uses_has_a_place(
        pieces in prop::collection::vec(piece(), 1..8),
        has_workspace: bool,
        has_home: bool,
    ) {
        let text: String = pieces
            .iter()
            .map(|piece| match piece {
                Piece::Text(text) => text.clone(),
                Piece::Place(placeholder) => placeholder.to_string(),
            })
            .collect();
        let mut places = Places::new(PathBuf::from(dir(Placeholder::Checkout)));
        if has_workspace {
            places = places.workspace(
                "w".parse().unwrap(),
                "a".parse().unwrap(),
                PathBuf::from(dir(Placeholder::Workspace)),
            );
        }
        if has_home {
            places = places.home(PathBuf::from(dir(Placeholder::Home)));
        }
        let has = |placeholder: Placeholder| match placeholder {
            Placeholder::Workspace => has_workspace,
            Placeholder::Home => has_home,
            _ => true,
        };
        let expected = pieces
            .iter()
            .map(|piece| match piece {
                Piece::Text(text) => Ok(text.as_str()),
                Piece::Place(placeholder) if has(*placeholder) => Ok(dir(*placeholder)),
                Piece::Place(placeholder) => Err(*placeholder),
            })
            .collect::<Result<String, Placeholder>>()
            .map(PathBuf::from);
        prop_assert_eq!(expand(&template(&text), &places), expected);
    }
}
