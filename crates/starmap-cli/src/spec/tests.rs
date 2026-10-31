use std::fs;
use std::path::Path;

use crumbkit::Failure as _;
use starmap::Spec;

use super::{SpecFailure, load};

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

// An uncapped read of /dev/zero never ends, so this test fails by hanging
// or running out of memory when the read loses its cap (BASE-34).
#[cfg(unix)]
#[test]
fn an_endless_file_is_too_large_because_the_read_stops_past_the_limit() {
    let failure = load(Path::new("/dev/zero")).unwrap_err();
    assert!(matches!(failure, SpecFailure::TooLarge { .. }));
    assert_eq!(failure.to_string(), "/dev/zero is over 256 KiB");
}

#[test]
fn a_file_with_one_problem_states_the_problem_in_its_message() {
    let failure = load(Path::new("../starmap/tests/spec/bad_default.toml")).unwrap_err();
    assert!(
        failure
            .to_string()
            .starts_with("../starmap/tests/spec/bad_default.toml: `variables.SHELF_PAGE_SIZE"),
        "{failure}"
    );
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
