//! The one module of the `starmap` command that reads files (BASE-5).

use std::fs::File;
use std::io::{self, Read as _};
use std::path::Path;

#[cfg(test)]
mod tests;

/// Reads at most `limit` bytes from the start of the file at `path`, so a
/// file of any size costs at most `limit` bytes of memory (BASE-34). A
/// caller that passes one byte more than it accepts learns that the file is
/// too large from the length of what it gets back.
///
/// # Errors
///
/// Returns the I/O error of a file that cannot be opened or read, such as a
/// missing file or a directory.
pub(crate) fn read_capped(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let limit = u64::try_from(limit).unwrap_or(u64::MAX);
    let mut bytes = Vec::new();
    File::open(path)?.take(limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}
