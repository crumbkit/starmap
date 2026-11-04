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
//! comment does not. A word after a `.` is a method of some other type, so
//! `ctx.var("X")` does not count. A full parse with syn would add nothing
//! that this shape needs, because the scan never asks what an expression
//! means.
//!
//! The scan leaves out test code, because a test may read a variable that
//! the application never reads. It skips each file named `tests.rs` or
//! ending in `_tests.rs`, where the Rust standards put unit tests, the
//! item that follows `#[cfg(test)]`, up to its closing brace, a semicolon
//! or a comma, and the rest of a module after `#![cfg(test)]`, so a file
//! that begins with `#![cfg(test)]` is skipped whole. The comma ends a
//! test-only field, variant, match arm or argument. Angle brackets are not
//! token groups, so any comma outside brackets in a test-only item's header
//! ends the skip too: in a function's generics such as `fn f<A, B>()`, in
//! a return type such as `-> Result<(), E>`, in `impl<A, B>`, in a `where`
//! clause, or in a `static` whose type is generic. The reads in the rest of
//! that item then count, so the scan over-counts them, and `check` reports
//! each one that the spec does not declare. The skip knows only the exact
//! attribute `cfg(test)`, so code under `cfg(all(test, unix))` counts.
//!
//! The scan misses a read whose name is not a literal at the call, such as
//! `env::var(NAME)` with a constant, and every lookup in the map that
//! `std::env::vars_os` returns. It also misses a variable that a library
//! reads for the application, such as clap's `#[arg(env = "X")]`, and a
//! reader that matches names by prefix. It counts any call of a function
//! named `var` or `var_os` with a literal name, including one from another
//! crate. `starmap check` then reports a missed variable as declared and
//! unread, and a miscounted one as undeclared. The application's own
//! `starmap_test::check` test still compares the derived struct with the
//! spec.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::Path;
use std::str::FromStr as _;

use crumbkit::{Failure, Recovery};
use proc_macro2::{Delimiter, Group, TokenStream, TokenTree};
use starmap::Name;

use crate::codes::SOURCE_UNREADABLE;
use crate::files;

#[cfg(test)]
mod tests;

/// The most bytes of one source file that the scan reads. A larger file,
/// such as generated bindings, fails with `starmap::source::unreadable`,
/// so it costs at most this much memory (BASE-34). The scan does not skip
/// such a file, because a skipped file could hide a read.
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
    for path in paths.into_iter().filter(|path| !is_test_file(path)) {
        let bytes = files::read_capped(&path, MAX_SOURCE_BYTES + 1)
            .map_err(|error| SourceFailure::new(&path, format!("cannot be read: {error}")))?;
        if bytes.len() > MAX_SOURCE_BYTES {
            return Err(SourceFailure::too_large(&path));
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

/// Reports whether `path` names a file of unit tests: `tests.rs`, or a
/// name that ends in `_tests.rs`.
fn is_test_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "tests.rs" || name.ends_with("_tests.rs"))
}

/// Adds each read in `tokens`, and in every bracket inside them, to
/// `reads`. It skips the item that follows `#[cfg(test)]`, and the rest of
/// the stream after an inner `#![cfg(test)]`.
fn collect(tokens: TokenStream, reads: &mut Vec<(Name, usize)>) {
    let mut before_previous: Option<TokenTree> = None;
    let mut previous: Option<TokenTree> = None;
    let mut reader = None;
    let mut in_test_item = false;
    for token in tokens {
        if in_test_item {
            in_test_item = !ends_item(&token);
            continue;
        }
        if let TokenTree::Group(group) = &token
            && is_cfg_test(group)
        {
            if is_punct(previous.as_ref(), '#') {
                in_test_item = true;
                previous = None;
                continue;
            }
            // An inner `#![cfg(test)]` makes the rest of its module test
            // code.
            if is_punct(previous.as_ref(), '!') && is_punct(before_previous.as_ref(), '#') {
                return;
            }
        }
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
            TokenTree::Ident(word)
                if READERS.iter().any(|reader| word == reader)
                    && !is_punct(previous.as_ref(), '.') =>
            {
                Some(word.span().start().line)
            }
            _ => None,
        };
        before_previous = previous.replace(token);
    }
}

/// Reports whether `token` is the punctuation `character`.
fn is_punct(token: Option<&TokenTree>, character: char) -> bool {
    matches!(token, Some(TokenTree::Punct(punct)) if punct.as_char() == character)
}

/// Reports whether an attribute's brackets hold `cfg(test)`.
fn is_cfg_test(group: &Group) -> bool {
    group.delimiter() == Delimiter::Bracket
        && group.stream().to_string().replace(' ', "") == "cfg(test)"
}

/// Reports whether `token` ends an item: its braces, a semicolon, or the
/// comma after a field, a variant, a match arm or an argument. The end of
/// the enclosing bracket ends the item too, because the stream ends there.
fn ends_item(token: &TokenTree) -> bool {
    match token {
        TokenTree::Group(group) => group.delimiter() == Delimiter::Brace,
        TokenTree::Punct(punct) => matches!(punct.as_char(), ';' | ','),
        TokenTree::Ident(_) | TokenTree::Literal(_) => false,
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
    recovery = Recovery::Hint(next.clone()),
    field(file = file.as_str()),
)]
pub(crate) struct SourceFailure {
    /// The path, as the scan found it.
    file: String,
    /// What is wrong with it, such as `is not valid Rust at line 3`.
    problem: String,
    /// The next step.
    next: String,
}

impl SourceFailure {
    /// Returns the failure of the file at `path`, which `problem` states.
    fn new(path: &Path, problem: String) -> Self {
        let file = path.display().to_string();
        let next = format!("fix {file} so that cargo builds it, or move it out of src");
        Self {
            file,
            problem,
            next,
        }
    }

    /// Returns the failure of the file at `path`, which is over 1 MiB.
    /// Such a file is usually generated and builds, so its next step is to
    /// move it.
    fn too_large(path: &Path) -> Self {
        Self {
            file: path.display().to_string(),
            problem: "is over 1 MiB, so the scan does not read it".to_owned(),
            next: "move generated code out of src".to_owned(),
        }
    }
}

impl fmt::Display for SourceFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.file, self.problem)
    }
}

impl std::error::Error for SourceFailure {}
