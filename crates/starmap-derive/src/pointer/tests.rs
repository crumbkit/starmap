//! Tests for the derive's read of a pointer file.

use std::collections::HashMap;

use super::{Followed, follow};

/// The text of a spec that declares one variable.
const DECLARES: &str = "[variables.SHELF_PORT]\npurpose = \"The port.\"\ntype = \"port\"\nunset = \"required\"\nshow = \"shown\"\nsource = \"setting\"\n";

/// Follows `path` among `files`, each a path relative to the crate root
/// and its text.
fn follow_in(path: &str, files: &[(&str, &str)]) -> Result<Followed, String> {
    let files: HashMap<&str, &str> = files.iter().copied().collect();
    follow(path, |path| files.get(path).map(|text| (*text).to_owned()))
}

#[test]
fn a_file_that_declares_variables_is_embedded_itself() {
    let followed = follow_in("starmap.toml", &[("starmap.toml", DECLARES)]).unwrap();

    assert_eq!(
        followed,
        Followed {
            file: "starmap.toml".to_owned(),
            pointer: None,
        }
    );
}

#[test]
fn a_missing_or_invalid_file_is_embedded_itself_so_the_usual_errors_report_it() {
    assert_eq!(follow_in("starmap.toml", &[]).unwrap().file, "starmap.toml");
    assert_eq!(
        follow_in("starmap.toml", &[("starmap.toml", "[variables")])
            .unwrap()
            .file,
        "starmap.toml"
    );
}

#[test]
fn a_pointer_embeds_the_file_it_names_relative_to_its_own_directory() {
    let followed = follow_in(
        "app/starmap.toml",
        &[
            ("app/starmap.toml", "spec = \"config/environment.toml\"\n"),
            ("app/config/environment.toml", DECLARES),
        ],
    )
    .unwrap();

    assert_eq!(
        followed,
        Followed {
            file: "app/config/environment.toml".to_owned(),
            pointer: Some("app/starmap.toml".to_owned()),
        }
    );
}

#[test]
fn a_pointer_that_declares_variables_fails_to_compile() {
    let error = follow_in(
        "starmap.toml",
        &[(
            "starmap.toml",
            &format!("spec = \"config/environment.toml\"\n{DECLARES}"),
        )],
    )
    .unwrap_err();

    assert!(error.contains("starmap::spec::pointer_declares"), "{error}");
}

#[test]
fn a_pointer_to_another_pointer_fails_to_compile() {
    let error = follow_in(
        "starmap.toml",
        &[
            ("starmap.toml", "spec = \"config/environment.toml\"\n"),
            ("config/environment.toml", "spec = \"shelf.toml\"\n"),
        ],
    )
    .unwrap_err();

    assert!(error.contains("starmap::spec::pointer_chain"), "{error}");
}

#[test]
fn a_pointer_whose_spec_is_not_a_path_fails_to_compile() {
    let error = follow_in("starmap.toml", &[("starmap.toml", "spec = 1\n")]).unwrap_err();

    assert_eq!(error, "`spec` in starmap.toml takes a path");
}

#[test]
fn a_pointer_to_a_parent_directory_names_the_file_without_dot_dot() {
    let followed = follow_in(
        "tests/pointer/starmap.toml",
        &[
            (
                "tests/pointer/starmap.toml",
                "spec = \"../example/starmap.toml\"\n",
            ),
            ("tests/example/starmap.toml", DECLARES),
        ],
    )
    .unwrap();

    assert_eq!(followed.file, "tests/example/starmap.toml");
}

#[test]
fn a_pointer_with_a_tool_table_embeds_the_file_it_names() {
    let followed = follow_in(
        "starmap.toml",
        &[
            (
                "starmap.toml",
                "spec = \"config/environment.toml\"\n\n[tool.codegen]\nmodule = \"settings\"\n",
            ),
            ("config/environment.toml", DECLARES),
        ],
    )
    .unwrap();

    assert_eq!(followed.file, "config/environment.toml");
}
