//! The derive's own read of a pointer file. starmap-derive cannot depend on
//! starmap, which depends on it, so this module reads only what the derive
//! needs: whether the file is a pointer, the file it names, and the two
//! refusals. starmap's `Pointer::parse` is the full parser, and the
//! derive's embedded spec still goes through `Spec::parse` at run time.
//!
//! The derive therefore accepts a pointer that the CLI refuses: one with a
//! key other than `spec`, `src` and `dotenv`, or with a `src` or `dotenv`
//! that is not a path. The derive reads neither `src` nor `dotenv`, so such
//! a pointer still embeds the right file, and `starmap check` reports its
//! problem.

use std::fs::File;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use toml_edit::{Document, TableLike};

#[cfg(test)]
mod tests;

/// The most bytes the derive reads of a file, which is the limit of
/// starmap's `Spec::MAX_BYTES`, 256 KiB. A larger file is not followed,
/// and `Spec::parse` refuses it at run time.
const MAX_BYTES: u64 = 256 * 1024;

/// The top-level keys of a file that declares variables. A pointer that
/// holds one of them also declares, which starmap refuses.
const DECLARING_FIELDS: &[&str] = &["mode", "variables"];

/// The file whose text the derive embeds.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Followed {
    /// The file that declares the variables, relative to the crate root
    /// unless it is absolute.
    pub(crate) file: String,
    /// The pointer that names `file`, relative to the crate root, when the
    /// struct's file is a pointer.
    pub(crate) pointer: Option<String>,
}

/// Returns the file to embed for the struct's file `path`, relative to the
/// crate root. `read` returns the text of a file relative to the crate
/// root, or `None` when it cannot read it. A file that cannot be read, is
/// not TOML or has no `spec` key is embedded itself, so `include_str!` and
/// `Spec::parse` report its problems as they do for any spec.
///
/// # Errors
///
/// Returns the message of the compile error for a pointer that also
/// declares variables, whose `spec` is not a path, or that names another
/// pointer.
pub(crate) fn follow(
    path: &str,
    read: impl Fn(&str) -> Option<String>,
) -> Result<Followed, String> {
    let itself = || Followed {
        file: path.to_owned(),
        pointer: None,
    };
    let Some(text) = read(path) else {
        return Ok(itself());
    };
    let Ok(document) = Document::parse(text.as_str()) else {
        return Ok(itself());
    };
    let root = document.as_table();
    let Some(spec) = root.get("spec") else {
        return Ok(itself());
    };
    if let Some(key) = declaring_key(root) {
        return Err(format!(
            "{path} has `spec`, so it points to another file, and it has `{key}` too, and a \
             file either points or declares variables (starmap::spec::pointer_declares)"
        ));
    }
    let target = match spec.as_str() {
        Some(target) if !target.is_empty() => join(path, target),
        _ => return Err(format!("`spec` in {path} takes a path")),
    };
    let points = read(&target)
        .and_then(|text| {
            Document::parse(text.as_str())
                .ok()
                .map(|target| target.as_table().contains_key("spec"))
        })
        .unwrap_or(false);
    if points {
        return Err(format!(
            "{path} points to {target}, which points to another file too, and starmap \
             follows one pointer (starmap::spec::pointer_chain)"
        ));
    }
    Ok(Followed {
        file: target,
        pointer: Some(path.to_owned()),
    })
}

/// Returns the first top-level key of `root` that declares variables.
fn declaring_key(root: &dyn TableLike) -> Option<&'static str> {
    DECLARING_FIELDS
        .iter()
        .copied()
        .find(|key| root.contains_key(key))
}

/// Returns `target`, which the pointer at `pointer` names relative to its
/// own directory, relative to the crate root. A `..` drops the directory
/// before it, so `config/../starmap.toml` reads as `starmap.toml`, and an
/// absolute `target` stays as it is.
fn join(pointer: &str, target: &str) -> String {
    let dir = Path::new(pointer).parent().unwrap_or(Path::new(""));
    let mut joined = PathBuf::new();
    for component in dir.join(target).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir
                if matches!(joined.components().next_back(), Some(Component::Normal(_))) =>
            {
                joined.pop();
            }
            other => joined.push(other),
        }
    }
    joined.to_string_lossy().into_owned()
}

/// Returns the text of the file at `path` under the crate root `root`, or
/// `None` when it cannot be read, is over [`MAX_BYTES`] or is not UTF-8.
pub(crate) fn read(root: &Path, path: &str) -> Option<String> {
    let mut bytes = Vec::new();
    File::open(root.join(path))
        .ok()?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > usize::try_from(MAX_BYTES).ok()? {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Returns the crate root of the crate that the derive expands in, which
/// Cargo passes to the compiler as `CARGO_MANIFEST_DIR`.
pub(crate) fn crate_root() -> Option<PathBuf> {
    #[expect(
        clippy::disallowed_methods,
        reason = "a proc macro learns the crate root it reads the pointer from only from CARGO_MANIFEST_DIR, which Cargo sets for the compiler"
    )]
    let root = std::env::var_os("CARGO_MANIFEST_DIR");
    root.map(PathBuf::from)
}
