//! Tests for the Rust file length check.

use super::check;
use crate::check::{File, Violation};

fn file(path: &str, lines: usize) -> File {
    File {
        path: path.to_owned(),
        contents: "let x = 1;\n".repeat(lines),
    }
}

#[test]
fn a_production_file_of_exactly_1000_lines_passes() {
    assert_eq!(check(&[file("crates/starmap/src/spec.rs", 1000)]), []);
}

#[test]
fn a_production_file_of_1001_lines_is_reported_with_its_count_and_the_limit() {
    assert_eq!(
        check(&[file("crates/starmap/src/spec.rs", 1001)]),
        [Violation::OverLimit {
            path: "crates/starmap/src/spec.rs".to_owned(),
            measured: 1001,
            unit: "lines",
            limit: 1000,
        }]
    );
}

#[test]
fn a_unit_test_file_has_no_limit() {
    assert_eq!(check(&[file("crates/starmap/src/spec/tests.rs", 5000)]), []);
}

#[test]
fn a_file_under_a_tests_directory_has_no_limit() {
    assert_eq!(check(&[file("crates/starmap-cli/tests/run.rs", 5000)]), []);
}

#[test]
fn a_file_under_a_benches_directory_has_no_limit() {
    assert_eq!(check(&[file("benches/benches/load.rs", 5000)]), []);
}

#[test]
fn a_long_file_that_is_not_rust_is_not_checked() {
    assert_eq!(check(&[file("CHANGELOG.md", 5000)]), []);
}
