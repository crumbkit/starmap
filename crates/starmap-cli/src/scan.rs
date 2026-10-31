//! The scan of an application's `src` directory for the environment
//! variables that its code reads, which `starmap init` guesses from and
//! `starmap check` compares with the spec.
//!
//! The scan splits each `.rs` file into Rust tokens with proc-macro2 and
//! looks for one shape: the word `var`, `var_os` or `starmap` followed by
//! parentheses that hold only a string literal, such as
//! `std::env::var("SHELF_PORT")` or the derive's field attribute
//! `#[starmap("SHELF_PORT")]`. It looks inside every bracket, so a read
//! inside a macro call counts, and comments are not tokens, so a name in a
//! comment does not. A full parse with syn would add nothing that this
//! shape needs, because the scan never asks what an expression means.
//!
//! The scan misses a read whose name is not a literal at the call, such as
//! `env::var(NAME)` with a constant, and every lookup in the map that
//! `std::env::vars_os` returns. `starmap check` then reports such a
//! variable as declared and unread, and the application's own
//! `starmap_test::check` test still compares the derived struct with the
//! spec.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::Path;
use std::str::FromStr as _;

use crumbkit::Failure;
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use starmap::Name;

use crate::codes::SOURCE_UNREADABLE;
use crate::files;

#[cfg(test)]
mod tests;

/// The most bytes of one source file that the scan reads. A larger file
/// fails with `starmap::source::unreadable`, so a stray generated file
/// costs at most this much memory (BASE-34).
const MAX_SOURCE_BYTES: usize = 1024 * 1024;

/// The words that read a variable when a string literal in parentheses
/// follows them: the two functions of `std::env` and the field attribute of
/// `#[derive(starmap::Env)]`.
const READERS: &[&str] = &["var", "var_os", "starmap"];

/// Each variable that the source reads, with the places that read it, such
/// as `src/serve.rs:22`, in the order of the files and their lines.
pub(crate) type Reads = BTreeMap<Name, Vec<String>>;

/// Reads every `.rs` file under `src` and returns the variables that they
/// read. A missing `src` has no reads. Each place names its file by `src`
/// joined with the file's path below it.
///
/// # Errors
///
/// Returns [`SourceFailure`] for a directory or file that cannot be read,
/// a file over 1 MiB, and a file that is not UTF-8 text or not Rust tokens.
pub(crate) fn scan(src: &Path) -> Result<Reads, SourceFailure> {
    let paths = match files::rust_files(src) {
        Ok(paths) => paths,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Reads::new()),
        Err(error) => return Err(SourceFailure::new(src, format!("cannot be read: {error}"))),
    };
    let mut reads = Reads::new();
    for path in paths {
        let bytes = files::read_capped(&path, MAX_SOURCE_BYTES + 1)
            .map_err(|error| SourceFailure::new(&path, format!("cannot be read: {error}")))?;
        if bytes.len() > MAX_SOURCE_BYTES {
            return Err(SourceFailure::new(&path, "is over 1 MiB".to_owned()));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| SourceFailure::new(&path, "is not UTF-8 text".to_owned()))?;
        let lines = lines_of_reads(&text).map_err(|error| {
            let line = error.span().start().line;
            SourceFailure::new(&path, format!("is not valid Rust at line {line}"))
        })?;
        for (name, line) in lines {
            let place = format!("{}:{line}", path.display());
            reads.entry(name).or_default().push(place);
        }
    }
    Ok(reads)
}

/// Returns each read in the Rust source `text` as its name and the line of
/// the word that reads it, in the order of the text.
///
/// # Errors
///
/// Returns proc-macro2's error for text that is not Rust tokens, such as an
/// unclosed bracket or string.
fn lines_of_reads(text: &str) -> Result<Vec<(Name, usize)>, proc_macro2::LexError> {
    let mut reads = Vec::new();
    collect(TokenStream::from_str(text)?, &mut reads);
    Ok(reads)
}

/// Adds each read in `tokens`, and in every bracket inside them, to
/// `reads`.
fn collect(tokens: TokenStream, reads: &mut Vec<(Name, usize)>) {
    let mut reader = None;
    for token in tokens {
        if let TokenTree::Group(group) = &token {
            if let Some(line) = reader
                && group.delimiter() == Delimiter::Parenthesis
                && let Some(name) = literal_name(group.stream())
            {
                reads.push((name, line));
            }
            collect(group.stream(), reads);
        }
        reader = match &token {
            TokenTree::Ident(word) if READERS.iter().any(|reader| word == reader) => {
                Some(word.span().start().line)
            }
            _ => None,
        };
    }
}

/// Returns the variable name that `tokens` hold when they are one string
/// literal, with or without a trailing comma, whose text is a valid name.
fn literal_name(tokens: TokenStream) -> Option<Name> {
    let mut tokens = tokens.into_iter();
    let Some(TokenTree::Literal(literal)) = tokens.next() else {
        return None;
    };
    match tokens.next() {
        None => {}
        Some(TokenTree::Punct(comma)) if comma.as_char() == ',' && tokens.next().is_none() => {}
        Some(_) => return None,
    }
    // A name holds no quote and no backslash, so its literal is the name
    // between quotes, or between a raw string's `r`, hashes and quotes.
    let written = literal.to_string();
    let text = written
        .strip_prefix('r')
        .unwrap_or(&written)
        .trim_matches('#');
    let name = text.strip_prefix('"')?.strip_suffix('"')?;
    Name::from_str(name).ok()
}

/// A source file or directory that the scan cannot read.
#[derive(Debug, Failure)]
#[failure(
    code = SOURCE_UNREADABLE,
    hint = "fix {file} so that cargo builds it, or move it out of src",
    field(file = file.as_str()),
)]
pub(crate) struct SourceFailure {
    /// The path, as the scan found it.
    file: String,
    /// What is wrong with it, such as `is over 1 MiB`.
    problem: String,
}

impl SourceFailure {
    /// Returns the failure of the file at `path`, which `problem` states.
    fn new(path: &Path, problem: String) -> Self {
        Self {
            file: path.display().to_string(),
            problem,
        }
    }
}

impl fmt::Display for SourceFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.file, self.problem)
    }
}

impl std::error::Error for SourceFailure {}
