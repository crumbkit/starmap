//! The AGENTS.md word check: an `AGENTS.md` file has at most 200 words, the
//! limit that the map's `agents-words` decision sets.

use std::ffi::OsStr;
use std::path::Path;

use super::{File, Violation};

#[cfg(test)]
mod tests;

/// The largest number of whitespace-separated words an `AGENTS.md` may have.
const LIMIT: usize = 200;

/// Reports each `AGENTS.md` file in `files`, in any directory, that has more
/// than 200 words.
pub(super) fn check(files: &[File]) -> Vec<Violation> {
    files
        .iter()
        .filter(|file| Path::new(&file.path).file_name() == Some(OsStr::new("AGENTS.md")))
        .filter_map(|file| {
            let measured = file.contents.split_whitespace().count();
            (measured > LIMIT).then(|| Violation::OverLimit {
                path: file.path.clone(),
                measured,
                unit: "words",
                limit: LIMIT,
            })
        })
        .collect()
}
