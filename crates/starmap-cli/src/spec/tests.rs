use std::fs;
use std::path::Path;

use crumbkit::Failure as _;
use starmap::Spec;

use super::{SpecFailure, load};

/// The directory that holds a `starmap.toml` fixture for each kind of spec
/// problem, relative to this package.
const SPEC_FIXTURES: &str = "../starmap/tests/spec";

#[test]
fn each_spec_problem_reports_the_code_of_its_kind() {
    let mut checked = 0;
    for entry in fs::read_dir(SPEC_FIXTURES).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "toml") {
            continue;
        }
        let kind = path.file_stem().unwrap().to_str().unwrap().to_owned();
        let failure = load(&path).unwrap_err();
        assert_eq!(failure.code().area(), "spec", "{kind}");
        assert_eq!(failure.code().reason(), kind, "{kind}");
        checked += 1;
    }
    assert!(checked >= 16, "only {checked} fixtures were checked");
}

#[test]
fn a_file_over_the_limit_that_ends_inside_a_character_is_too_large() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("starmap.toml");
    // The read stops one byte past the limit, inside the two-byte é.
    let mut text = "#".repeat(Spec::MAX_BYTES);
    text.push('é');
    fs::write(&path, text).unwrap();
    assert_eq!(load(&path).unwrap_err().code().reason(), "too_large");
}

#[test]
fn a_missing_file_is_not_found_and_a_directory_is_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        load(&dir.path().join("starmap.toml")),
        Err(SpecFailure::NotFound { .. })
    ));
    assert!(matches!(
        load(dir.path()),
        Err(SpecFailure::Unreadable { .. })
    ));
}

#[test]
fn the_example_spec_loads() {
    let spec = load(Path::new("../starmap/tests/example/starmap.toml")).unwrap();
    assert_eq!(spec.variables().count(), 9);
}
