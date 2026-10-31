//! The one module of the `starmap` command that reads and writes files
//! (BASE-5).

use std::fs::{self, File};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};

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

/// Returns the path of every regular file whose name ends in `.rs` under
/// `dir`, in path order. The walk follows no symbolic link, so a link
/// cycle cannot hold it, and a file reached only through a link is left
/// out.
///
/// # Errors
///
/// Returns the I/O error of a directory that cannot be listed, such as a
/// missing `dir`.
pub(crate) fn rust_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_dir() {
                dirs.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Writes `text` to a new file at `path`, and keeps a file that is already
/// there unchanged.
///
/// # Errors
///
/// Returns the I/O error of a file that cannot be created or written, and
/// `AlreadyExists` when `path` names an existing file.
pub(crate) fn write_new(path: &Path, text: &str) -> io::Result<()> {
    let mut file = File::options().write(true).create_new(true).open(path)?;
    file.write_all(text.as_bytes())
}

/// Reports whether anything is at `path`. A path that cannot be checked,
/// such as one below a directory that cannot be read, counts as free, and
/// the write that follows reports why it fails.
pub(crate) fn exists(path: &Path) -> bool {
    path.exists()
}
