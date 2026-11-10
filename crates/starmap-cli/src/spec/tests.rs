use std::fs;
use std::path::Path;

use crumbkit::{Failure as _, Spelling};
use starmap::Spec;

use super::{SpecFailure, open};

/// Reads and parses the spec at `path`, through its pointer when it holds
/// one.
fn load(path: &Path) -> Result<Spec, SpecFailure> {
    open(path).map(|opened| opened.spec)
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

#[test]
fn each_problem_fixture_fails_with_the_code_its_first_problem_kind_names() {
    let mut checked = 0;
    for entry in fs::read_dir("../starmap/tests/spec").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "toml") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let kind = Spec::parse(&text).unwrap_err().problems()[0].kind();
        let failure = load(&path).unwrap_err();
        assert_eq!(
            Spelling::COLONS.spell(failure.code()),
            kind.code(),
            "{}",
            path.display()
        );
        checked += 1;
    }
    assert!(checked > 10, "{checked}");
}

#[test]
fn a_spec_that_cannot_be_written_or_read_offers_another_file_as_a_second_repair() {
    let error = || std::io::Error::other("refused");
    let unwritable = SpecFailure::Unwritable {
        file: "app/starmap.toml".to_owned(),
        error: error(),
    };
    let unreadable = SpecFailure::Unreadable {
        file: "app/starmap.toml".to_owned(),
        error: error(),
    };
    let another = vec![crumbkit::Recovery::hint("pass --spec another file")];
    assert_eq!(
        unwritable.recovery(),
        crumbkit::Recovery::hint("make the directory of app/starmap.toml writable")
    );
    assert_eq!(unwritable.alternatives(), another);
    assert_eq!(
        unreadable.recovery(),
        crumbkit::Recovery::hint("make app/starmap.toml readable")
    );
    assert_eq!(unreadable.alternatives(), another);
}
