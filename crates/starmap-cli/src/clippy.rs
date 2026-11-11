//! The Clippy entries that stop an application's own code from reading the
//! environment outside its struct that derives `starmap::Env`. `starmap
//! init` adds them to each clippy.toml that Clippy reads for a crate of the
//! application, and `starmap check` fails when one is missing.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crumbkit::{Failure, Recovery};
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, RawString, Table, Value};

use crate::codes::{CLIPPY_INVALID, CLIPPY_UNWRITABLE};
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

/// A clippy.toml that Clippy reads for a crate of the application, and the
/// banned methods it does not disallow yet.
#[derive(Debug)]
pub(crate) struct Ban {
    /// The file, or the file that init writes when Clippy finds none.
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
    /// written. `spec` is the `--spec` path, which the failure's recovery
    /// names.
    ///
    /// # Errors
    ///
    /// Returns [`ClippyFailure::Unwritable`] when the file cannot be
    /// written, and the file is then as it was.
    pub(crate) fn write(mut self, spec: &Path) -> Result<(), ClippyFailure> {
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
            spec: spec.display().to_string(),
            error,
        })
    }

    /// Returns the sentence that says what init did to the file, or what
    /// it would do when `dry_run` is set.
    fn sentence(&self, dry_run: bool) -> String {
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

/// Returns one line for each file of `bans` that says what init did to it,
/// or what it would do when `dry_run` is set. When `bans` is empty, the
/// scan read no `.rs` file, and the one line says that init wrote no file.
pub(crate) fn sentences(bans: &[Ban], dry_run: bool) -> String {
    if bans.is_empty() {
        return "init read no .rs file, so it wrote no clippy.toml.".to_owned();
    }
    let lines: Vec<String> = bans.iter().map(|ban| ban.sentence(dry_run)).collect();
    lines.join("\n")
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

/// Reads the clippy.toml that Clippy uses for the crate of each directory
/// of `dirs`, the directories that hold a `.rs` file that the scan read,
/// and returns each file once, with the banned methods it does not
/// disallow. The crate's directory is the nearest directory at or above
/// the source directory that holds a Cargo.toml, however far above, and
/// the directory of `spec` when none does. [`find`] states which file
/// Clippy reads for a crate. starmap does not read `CLIPPY_CONF_DIR`.
///
/// When [`find`] finds no file, the result names the `clippy.toml` that
/// init writes, which does not exist and lacks every entry. A file in or
/// below the directory of `spec` has a path that starts as `spec` does,
/// and any other file has an absolute path. A file that is a symbolic
/// link is named as the file that the link leads to.
///
/// A file is an `Err` when it cannot be read, is over 256 KiB, is not
/// UTF-8 text or valid TOML, or sets `disallowed-methods` to a value that
/// is not an array. A symbolic link that leads to no file cannot be read.
pub(crate) fn open(spec: &Path, dirs: &[PathBuf]) -> Vec<Result<Ban, ClippyFailure>> {
    let beside = spec.parent().unwrap_or(Path::new(""));
    let base = canonical(beside);
    let mut crates: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        let dir = canonical(dir);
        let krate = dir
            .ancestors()
            .find(|above| files::is_file(&above.join("Cargo.toml")))
            .unwrap_or(&base);
        if !crates.iter().any(|found| found == krate) {
            crates.push(krate.to_path_buf());
        }
    }
    let mut found: Vec<(PathBuf, bool)> = Vec::new();
    for krate in crates {
        let (file, exists) = find(&krate, &base);
        let file = match file.strip_prefix(&base) {
            Ok(below) => beside.join(below),
            Err(_) => file,
        };
        if !found.contains(&(file.clone(), exists)) {
            found.push((file, exists));
        }
    }
    found
        .into_iter()
        .map(|(file, exists)| read(file, exists))
        .collect()
}

/// Returns the absolute path of the directory `dir` with every symbolic
/// link resolved, where an empty path is the working directory. A
/// directory that cannot be resolved keeps the path given.
fn canonical(dir: &Path) -> PathBuf {
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    files::canonical(dir).unwrap_or_else(|_| dir.to_path_buf())
}

/// Returns the configuration file that Clippy reads for the crate in the
/// directory `krate`, and whether it exists. `krate` and `base`, the
/// directory of the spec, have every symbolic link resolved.
///
/// The search stays inside a root, which is the nearest directory at or
/// above `krate` that holds `.git`. Without one, the root is the nearest
/// directory at or above `krate` whose Cargo.toml has a `[workspace]`
/// table, and `krate` itself when none has. The file is the first of
/// [`NAMES`] in the crate's directory, or else in the nearest directory
/// from there up to the root that holds one. Clippy skips a directory that
/// has one of those names, and so does the search. Clippy also reads a
/// file above the root, which starmap never reads or edits.
///
/// When no directory holds a file, the file is the `clippy.toml` that init
/// writes: in `base` when `base` is the crate's directory or a directory
/// above it inside the root, and in the crate's directory otherwise. A
/// file that is a symbolic link is returned as the file that the link
/// leads to, when it leads to one.
fn find(krate: &Path, base: &Path) -> (PathBuf, bool) {
    let root = krate
        .ancestors()
        .find(|above| files::exists(&above.join(".git")))
        .or_else(|| krate.ancestors().find(|above| is_workspace(above)))
        .unwrap_or(krate);
    let found = krate
        .ancestors()
        .take_while(|above| above.starts_with(root))
        .flat_map(|dir| NAMES.map(|name| dir.join(name)))
        .find(|file| files::is_file(file));
    match found {
        Some(file) => (files::canonical(&file).unwrap_or(file), true),
        None => {
            let beside_spec = krate.starts_with(base) && base.starts_with(root);
            let dir = if beside_spec { base } else { krate };
            (dir.join("clippy.toml"), false)
        }
    }
}

/// Reports whether the Cargo.toml in `dir` has a `[workspace]` table,
/// which makes `dir` the root of a Cargo workspace. A Cargo.toml that
/// cannot be read or parsed, or is over 256 KiB, counts as having none, so
/// the search for a clippy.toml then stops nearer the crate, and Cargo
/// itself reports the manifest.
fn is_workspace(dir: &Path) -> bool {
    files::read_capped(&dir.join("Cargo.toml"), MAX_BYTES)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| text.parse::<DocumentMut>().ok())
        .is_some_and(|manifest| manifest.get("workspace").is_some_and(Item::is_table_like))
}

/// Reads and parses `file`, or returns a ban that lacks every entry when
/// the file does not exist.
fn read(file: PathBuf, exists: bool) -> Result<Ban, ClippyFailure> {
    if !exists {
        return Ok(Ban {
            file,
            exists,
            missing: BANNED.to_vec(),
            document: DocumentMut::new(),
        });
    }
    let invalid = |reason: String| ClippyFailure::Invalid {
        file: file.display().to_string(),
        reason,
    };
    let bytes = files::read_capped(&file, MAX_BYTES + 1)
        .map_err(|error| invalid(format!("cannot be read: {error}")))?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid("is over 256 KiB".to_owned()));
    }
    let text = String::from_utf8(bytes).map_err(|_| invalid("is not UTF-8 text".to_owned()))?;
    let document: DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| {
        invalid(format!("is not valid TOML: {}", error.message()))
    })?;
    let missing = missing(&document)
        .ok_or_else(|| invalid(format!("sets {KEY} to a value that is not an array")))?;
    Ok(Ban {
        file,
        exists,
        missing,
        document,
    })
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
    /// The file cannot be read, is over [`MAX_BYTES`], is not UTF-8 text
    /// or valid TOML, or its `disallowed-methods` is not an array.
    #[failure(
        code = CLIPPY_INVALID,
        hint = "make {file} a readable file of at most 256 KiB that holds valid TOML whose disallowed-methods is an array",
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
        hint = "make {file} and its directory writable, then run starmap init --spec {spec} --clippy",
        alternative = Recovery::Hint(format!("add the entries to {file} by hand, as starmap check lists them")),
        field(file = file.as_str()),
    )]
    Unwritable {
        /// The file.
        file: String,
        /// The file that the caller gave as `--spec`.
        spec: String,
        /// Why it cannot be written.
        error: io::Error,
    },
}

impl fmt::Display for ClippyFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { file, reason } => write!(formatter, "{file} {reason}"),
            Self::Unwritable { file, .. } => write!(formatter, "{file} cannot be written"),
        }
    }
}

/// A file that cannot be written has the I/O error as its source, which
/// prints as the failure's cause.
impl std::error::Error for ClippyFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unwritable { error, .. } => Some(error),
            Self::Invalid { .. } => None,
        }
    }
}
