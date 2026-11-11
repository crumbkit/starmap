//! The Clippy entries that stop an application's own code from reading the
//! environment outside its struct that derives `starmap::Env`. `starmap
//! init` adds them to the application's clippy.toml, and `starmap check`
//! fails when one is missing.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crumbkit::{Failure, Recovery};
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, RawString, Table, Value};

use crate::codes::{CLIPPY_INVALID, CLIPPY_UNREADABLE, CLIPPY_UNWRITABLE};
use crate::files;

#[cfg(test)]
mod tests;

/// The methods of the standard library that read the environment, each of
/// which the application's clippy.toml disallows.
pub(crate) const BANNED: [&str; 4] = [
    "std::env::var",
    "std::env::var_os",
    "std::env::vars",
    "std::env::vars_os",
];

/// The reason that Clippy prints beside a call of a banned method.
const REASON: &str = "read the environment through the struct that derives starmap::Env, whose variables starmap.toml declares";

/// The key of clippy.toml that lists the methods Clippy refuses.
const KEY: &str = "disallowed-methods";

/// The file names that Clippy reads its configuration from, in the order
/// it tries them in each directory.
const NAMES: [&str; 2] = [".clippy.toml", "clippy.toml"];

/// The most bytes that starmap reads of a clippy.toml (BASE-34).
const MAX_BYTES: usize = 256 * 1024;

/// The clippy.toml of an application, and the banned methods it does not
/// disallow yet.
#[derive(Debug)]
pub(crate) struct Ban {
    /// The file that Clippy reads for a crate beside the spec, or the file
    /// beside the spec that init writes when there is none.
    pub(crate) file: PathBuf,
    /// Whether the file exists.
    pub(crate) exists: bool,
    /// Each method of [`BANNED`] that the file does not disallow.
    pub(crate) missing: Vec<&'static str>,
    /// The file's parsed text, which is empty for a file that does not
    /// exist.
    document: DocumentMut,
}

impl Ban {
    /// Adds an entry for each missing method to the file, and creates the
    /// file when it does not exist. A file that lacks no entry is not
    /// written.
    ///
    /// # Errors
    ///
    /// Returns [`ClippyFailure::Unwritable`] when the file cannot be
    /// written, and the file is then as it was.
    pub(crate) fn write(mut self) -> Result<(), ClippyFailure> {
        if self.missing.is_empty() {
            return Ok(());
        }
        add(&mut self.document, &self.missing);
        let text = self.document.to_string();
        let written = if self.exists {
            files::replace(&self.file, &text)
        } else {
            files::write_new(&self.file, &text)
        };
        written.map_err(|error| ClippyFailure::Unwritable {
            file: self.file.display().to_string(),
            error,
        })
    }

    /// Returns the sentence that says what init did to the file, or what
    /// it would do when `dry_run` is set.
    pub(crate) fn sentence(&self, dry_run: bool) -> String {
        let file = self.file.display();
        let methods = list(&self.missing);
        match (self.missing.is_empty(), self.exists, dry_run) {
            (true, _, _) => format!("{file} already disallows {}.", list(&BANNED)),
            (false, true, false) => {
                format!("Added {KEY} entries to {file} for {methods}.")
            }
            (false, false, false) => format!("Wrote {file} with {KEY} entries for {methods}."),
            (false, true, true) => {
                format!("init would add {KEY} entries to {file} for {methods}.")
            }
            (false, false, true) => {
                format!("init would write {file} with {KEY} entries for {methods}.")
            }
        }
    }
}

/// Returns `methods` as a sentence lists them, such as `std::env::var and
/// std::env::vars`.
pub(crate) fn list(methods: &[&str]) -> String {
    match methods.split_last() {
        None => String::new(),
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// Reads the clippy.toml that Clippy uses for a crate beside the file
/// `spec`, and returns it with the banned methods it does not disallow.
/// Clippy reads `.clippy.toml` or `clippy.toml` in the crate's directory,
/// or else in the nearest directory above it that has one. When no
/// directory has one, the result names `clippy.toml` beside `spec`, which
/// does not exist and lacks every entry.
///
/// # Errors
///
/// Returns [`ClippyFailure`] when the file cannot be read, is over 256 KiB,
/// is not UTF-8 text or valid TOML, or sets `disallowed-methods` to a value
/// that is not an array.
pub(crate) fn open(spec: &Path) -> Result<Ban, ClippyFailure> {
    let dir = spec.parent().unwrap_or(Path::new(""));
    let Some(file) = find(dir) else {
        return Ok(Ban {
            file: dir.join("clippy.toml"),
            exists: false,
            missing: BANNED.to_vec(),
            document: DocumentMut::new(),
        });
    };
    let shown = file.display().to_string();
    let unreadable = |error| ClippyFailure::Unreadable {
        file: shown.clone(),
        error,
    };
    let invalid = |reason: String| ClippyFailure::Invalid {
        file: shown.clone(),
        reason,
    };
    let bytes = files::read_capped(&file, MAX_BYTES + 1).map_err(unreadable)?;
    if bytes.len() > MAX_BYTES {
        return Err(unreadable(io::Error::other("the file is over 256 KiB")));
    }
    let text = String::from_utf8(bytes).map_err(|_| invalid("is not UTF-8 text".to_owned()))?;
    let document: DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| {
        invalid(format!("is not valid TOML: {}", error.message()))
    })?;
    let missing = missing(&document)
        .ok_or_else(|| invalid(format!("sets {KEY} to a value that is not an array")))?;
    Ok(Ban {
        file,
        exists: true,
        missing,
        document,
    })
}

/// Returns the configuration file that Clippy reads for a crate in `dir`:
/// the first of [`NAMES`] in `dir`, or else in the nearest directory above
/// it that holds one. A file in `dir` keeps the path the caller gave, and
/// a file above it has an absolute path.
fn find(dir: &Path) -> Option<PathBuf> {
    let here = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    // A directory that does not exist has no directory above it to search.
    let above = files::canonical(here).unwrap_or_default();
    std::iter::once(dir)
        .chain(above.ancestors().skip(1))
        .flat_map(|dir| NAMES.map(|name| dir.join(name)))
        .find(|file| files::exists(file))
}

/// Returns each method of [`BANNED`] that `document` does not disallow, or
/// `None` when its `disallowed-methods` is not an array. An entry is a
/// string that holds the method's path, or a table whose `path` holds it,
/// as Clippy reads either.
fn missing(document: &DocumentMut) -> Option<Vec<&'static str>> {
    let listed: Vec<&str> = match document.get(KEY) {
        None => Vec::new(),
        Some(Item::Value(Value::Array(array))) => array
            .iter()
            .filter_map(|entry| match entry {
                Value::InlineTable(table) => table.get("path").and_then(Value::as_str),
                entry => entry.as_str(),
            })
            .collect(),
        Some(Item::ArrayOfTables(tables)) => tables
            .iter()
            .filter_map(|table| table.get("path").and_then(Item::as_str))
            .collect(),
        Some(_) => return None,
    };
    Some(
        BANNED
            .into_iter()
            .filter(|method| !listed.contains(method))
            .collect(),
    )
}

/// Adds an entry for each method of `methods` after the entries that
/// `document` lists already, in the form the file uses: an inline table in
/// an array, or a `[[disallowed-methods]]` table. A file without the key
/// gains an array with one entry on each line.
fn add(document: &mut DocumentMut, methods: &[&'static str]) {
    if methods.is_empty() {
        return;
    }
    match document
        .entry(KEY)
        .or_insert_with(|| Item::Value(Value::Array(Array::new())))
    {
        Item::Value(Value::Array(array)) => extend(array, methods),
        Item::ArrayOfTables(tables) => append(tables, methods),
        // `missing` returns `None` for any other item, so `open` refuses
        // the file before it reaches here.
        _ => {}
    }
}

/// Adds an inline table for each method of `methods` to `array`. An array
/// that is empty or has an entry on a line of its own gains each entry on
/// its own line, indented as the last entry is, and an array on one line
/// stays on one line. A comment beside the last entry stays beside it, and
/// a comment on a line of its own before the closing bracket stays there.
fn extend(array: &mut Array, methods: &[&'static str]) {
    let indent = array
        .iter()
        .last()
        .map_or(Some("\n    ".to_owned()), |last| {
            let prefix = last.decor().prefix()?.as_str()?;
            let (_, spaces) = prefix.rsplit_once('\n')?;
            Some(format!("\n{spaces}"))
        });
    let table = |method: &str| {
        let mut table = InlineTable::new();
        table.insert("path", method.into());
        table.insert("reason", REASON.into());
        Value::InlineTable(table)
    };
    let Some(indent) = indent else {
        for method in methods {
            array.push(table(method));
        }
        return;
    };
    // The text between the last entry and the closing bracket. Its first
    // line holds a comment beside the last entry, which stays on that line,
    // and its other lines stay before the closing bracket.
    let mut tail = String::new();
    if let Some(last) = array.iter_mut().last() {
        tail.push_str(text(last.decor().suffix()));
        last.decor_mut().set_suffix("");
    }
    tail.push_str(text(Some(array.trailing())));
    let (beside, below) = tail.split_once('\n').unwrap_or((&tail, ""));
    let mut prefix = format!("{}{indent}", beside.trim_end());
    for method in methods {
        let mut entry = table(method);
        entry.decor_mut().set_prefix(prefix.as_str());
        array.push_formatted(entry);
        prefix.clone_from(&indent);
    }
    array.set_trailing_comma(true);
    array.set_trailing(format!("\n{below}"));
}

/// Returns the text of `raw`, which is empty when there is none.
fn text(raw: Option<&RawString>) -> &str {
    raw.and_then(RawString::as_str).unwrap_or_default()
}

/// Adds a `[[disallowed-methods]]` table for each method of `methods` to
/// `tables`.
fn append(tables: &mut ArrayOfTables, methods: &[&'static str]) {
    for method in methods {
        let mut table = Table::new();
        table.insert("path", toml_edit::value(*method));
        table.insert("reason", toml_edit::value(REASON));
        tables.push(table);
    }
}

/// A clippy.toml that `starmap init` and `starmap check` refuse. Neither
/// command writes a file that it refuses.
#[derive(Debug, Failure)]
pub(crate) enum ClippyFailure {
    /// The file exists and cannot be read, or is over [`MAX_BYTES`].
    #[failure(
        code = CLIPPY_UNREADABLE,
        hint = "make {file} a readable text file of at most 256 KiB",
        field(file = file.as_str()),
    )]
    Unreadable {
        /// The file.
        file: String,
        /// Why it cannot be read.
        error: io::Error,
    },
    /// The file is not UTF-8 text or valid TOML, or its
    /// `disallowed-methods` is not an array.
    #[failure(
        code = CLIPPY_INVALID,
        hint = "correct {file} so that it is valid TOML and its disallowed-methods is an array",
        field(file = file.as_str()),
    )]
    Invalid {
        /// The file.
        file: String,
        /// What is wrong with it, as the words after the file's name.
        reason: String,
    },
    /// `init` cannot create or write the file.
    #[failure(
        code = CLIPPY_UNWRITABLE,
        hint = "make {file} and its directory writable",
        alternative = Recovery::Hint(format!("add the entries to {file} by hand, as starmap check lists them")),
        field(file = file.as_str()),
    )]
    Unwritable {
        /// The file.
        file: String,
        /// Why it cannot be written.
        error: io::Error,
    },
}

impl fmt::Display for ClippyFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { file, .. } => write!(formatter, "{file} cannot be read"),
            Self::Invalid { file, reason } => write!(formatter, "{file} {reason}"),
            Self::Unwritable { file, .. } => write!(formatter, "{file} cannot be written"),
        }
    }
}

/// A file that cannot be read or written has the I/O error as its source,
/// which prints as the failure's cause.
impl std::error::Error for ClippyFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable { error, .. } | Self::Unwritable { error, .. } => Some(error),
            Self::Invalid { .. } => None,
        }
    }
}
