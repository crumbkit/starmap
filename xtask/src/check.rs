//! The repository checks. Each check is a pure function from files to
//! violations, so its tests need no file system.

mod agents_words;
mod rust_lines;

use std::fmt;

/// A file in the repository.
pub(crate) struct File {
    /// The path relative to the repository root, with `/` separators.
    pub(crate) path: String,
    /// The file's text, or an empty string when the file is not UTF-8 text,
    /// such as an image.
    pub(crate) contents: String,
}

/// A problem that a check found in one file.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Violation {
    /// A measured quantity is over its limit.
    OverLimit {
        /// The file that was measured.
        path: String,
        /// The measured quantity.
        measured: usize,
        /// What the quantity counts, such as `lines`.
        unit: &'static str,
        /// The largest quantity that passes.
        limit: usize,
    },
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OverLimit {
                path,
                measured,
                unit,
                limit,
            } => write!(f, "{path}: {measured} {unit}, limit {limit}"),
        }
    }
}

/// Runs every check over `files` and returns the violations in the order
/// of the checks.
pub(crate) fn run(files: &[File]) -> Vec<Violation> {
    let mut violations = agents_words::check(files);
    violations.extend(rust_lines::check(files));
    violations
}
