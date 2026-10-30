//! The Rust file length check of BASE-21: a production `.rs` file has at most
//! 1,000 lines. Files that hold only tests have no limit.

use std::ffi::OsStr;
use std::path::{Component, Path};

use super::{File, Violation};

#[cfg(test)]
mod tests;

/// The largest number of lines a production `.rs` file may have.
const LIMIT: usize = 1000;

/// Reports each production `.rs` file in `files` that has more than 1,000
/// lines.
pub(super) fn check(files: &[File]) -> Vec<Violation> {
    files
        .iter()
        .filter(|file| is_production(Path::new(&file.path)))
        .filter_map(|file| {
            let measured = file.contents.lines().count();
            (measured > LIMIT).then(|| Violation::OverLimit {
                path: file.path.clone(),
                measured,
                unit: "lines",
                limit: LIMIT,
            })
        })
        .collect()
}

/// Whether `path` is a `.rs` file that holds production code. A unit test
/// file `foo/tests.rs` and any file under a `tests/` or `benches/` directory
/// hold only tests (BASE-21, BASE-22).
fn is_production(path: &Path) -> bool {
    let is_rust = path.extension() == Some(OsStr::new("rs"));
    let is_unit_tests = path.file_name() == Some(OsStr::new("tests.rs"));
    let in_test_directory = path.components().any(|component| {
        component == Component::Normal(OsStr::new("tests"))
            || component == Component::Normal(OsStr::new("benches"))
    });
    is_rust && !is_unit_tests && !in_test_directory
}
