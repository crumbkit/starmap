//! The scan of an application's `src` directory for the environment
//! variables that its code reads, which `starmap init` guesses from and
//! `starmap check` compares with the spec.
//!
//! The scan splits each `.rs` file into Rust tokens with proc-macro2 and
//! looks for one shape: the word `var`, `var_os` or `starmap` followed by
//! parentheses that hold only a string literal, such as
//! `std::env::var("SHELF_PORT")` or the derive's field attribute
//! `#[starmap("SHELF_PORT")]`. After `starmap`, the literal may be followed
//! by `, check = <path>`, as in `#[starmap("SHELF_PORT", check = unprivileged)]`,
//! and the scan records that the field names a check.
//!
//! After `var` or `var_os`, the parentheses may instead hold a `format!`
//! call, with or without a leading `&` and a path such as `std::format!`,
//! whose first argument is a string literal. The scan reads the literal as
//! `format!` does: `{{` and `}}` are braces, and any other `{...}` is a
//! placeholder. A literal that is a variable name, an underscore and one
//! placeholder at the end, such as `"SHELF_DB_URL_{region}"`, reads a
//! member of the family with that prefix. A literal without a placeholder
//! reads the name it holds. Any other literal, such as one that starts with
//! a placeholder, has one in the middle or has two, is unresolved, and the
//! scan records only its place. A field attribute's `format!` is not a
//! read.
//!
//! It looks inside every bracket, so a read
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
//! The scan misses a read whose name is not a literal or a `format!` at the
//! call, such as `env::var(NAME)` with a constant, a name built with
//! `concat!`, `+` or `push_str`, and every lookup in the map that
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
use std::path::{Path, PathBuf};
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

/// What a scan finds: the reads, each variable whose field attribute names
/// a check, with the first place that names it, the family reads and the
/// unresolved reads.
#[derive(Debug, Default)]
pub(crate) struct Scanned {
    /// Each variable that the source reads.
    pub(crate) reads: Reads,
    /// Each variable that a `#[starmap("NAME", check = <path>)]` field
    /// attribute reads, with its first place.
    pub(crate) checks: BTreeMap<Name, String>,
    /// Each family prefix whose members the source reads through a
    /// `format!` name, with the places that read it.
    pub(crate) families: Reads,
    /// The place of each read whose `format!` name the scan cannot
    /// resolve, in the order of the files and their lines.
    pub(crate) unresolved: Vec<String>,
}

/// What one read names.
#[derive(Debug)]
enum Target {
    /// The variable that a literal names.
    Variable(Name),
    /// A member of the family with this prefix, which a `format!` name
    /// with one trailing placeholder reads.
    Family(Name),
    /// A `format!` name that the scan cannot resolve.
    Unresolved,
}

/// One read in a file: what it names, the line of the word that reads it,
/// and whether it is a field attribute that names a check.
#[derive(Debug)]
struct Read {
    /// What the read names.
    target: Target,
    /// The line of the word that reads it.
    line: usize,
    /// Whether a `check = <path>` follows the name.
    checked: bool,
}

/// Reads every `.rs` file under each directory of `dirs`, as [`scan`]
/// does, and returns the reads of all of them. A variable that several
/// directories read lists the places of each directory in turn.
///
/// # Errors
///
/// Returns [`SourceFailure`] as [`scan`] does for any of the directories.
pub(crate) fn scan_all(dirs: &[PathBuf]) -> Result<Scanned, SourceFailure> {
    let mut all = Scanned::default();
    for dir in dirs {
        let scanned = scan(dir)?;
        for (name, places) in scanned.reads {
            all.reads.entry(name).or_default().extend(places);
        }
        for (name, at) in scanned.checks {
            all.checks.entry(name).or_insert(at);
        }
        for (prefix, places) in scanned.families {
            all.families.entry(prefix).or_default().extend(places);
        }
        all.unresolved.extend(scanned.unresolved);
    }
    Ok(all)
}

/// The source directories of a scan, which print as `src`, as `src and
/// bin` when there are several, and as `no source directory` when a
/// pointer lists none.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Dirs<'a>(pub(crate) &'a [PathBuf]);

impl Dirs<'_> {
    /// Returns whether there is no directory, so the scan reads nothing.
    pub(crate) fn is_empty(self) -> bool {
        self.0.is_empty()
    }

    /// Returns `one` when there is one directory, and `several` otherwise,
    /// so a sentence agrees with the directories it names.
    pub(crate) fn verb(self, one: &'static str, several: &'static str) -> &'static str {
        if self.0.len() == 1 { one } else { several }
    }
}

impl fmt::Display for Dirs<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<String> = self.0.iter().map(|dir| dir.display().to_string()).collect();
        match names.split_last() {
            None => formatter.write_str("no source directory"),
            Some((last, [])) => formatter.write_str(last),
            Some((last, rest)) => write!(formatter, "{} and {last}", rest.join(", ")),
        }
    }
}

/// Reads every `.rs` file under `src` and returns the variables that they
/// read. A missing `src` has no reads. Each place names its file by `src`
/// joined with the file's path below it.
///
/// # Errors
///
/// Returns [`SourceFailure`] for a directory or file that cannot be read,
/// a file over 1 MiB, and a file that is not UTF-8 text or not Rust tokens.
pub(crate) fn scan(src: &Path) -> Result<Scanned, SourceFailure> {
    let paths = match files::rust_files(src) {
        Ok(paths) => paths,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Scanned::default()),
        Err(error) => return Err(SourceFailure::new(src, format!("cannot be read: {error}"))),
    };
    let mut scanned = Scanned::default();
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
        for Read {
            target,
            line,
            checked,
        } in lines
        {
            let place = format!("{}:{line}", path.display());
            match target {
                Target::Variable(name) => {
                    if checked {
                        scanned
                            .checks
                            .entry(name.clone())
                            .or_insert_with(|| place.clone());
                    }
                    scanned.reads.entry(name).or_default().push(place);
                }
                Target::Family(prefix) => {
                    scanned.families.entry(prefix).or_default().push(place);
                }
                Target::Unresolved => scanned.unresolved.push(place),
            }
        }
    }
    Ok(scanned)
}

/// Returns each read in the Rust source `text`, in the order of the text.
///
/// # Errors
///
/// Returns proc-macro2's error for text that is not Rust tokens, such as an
/// unclosed bracket or string.
fn lines_of_reads(text: &str) -> Result<Vec<Read>, proc_macro2::LexError> {
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
fn collect(tokens: TokenStream, reads: &mut Vec<Read>) {
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
            if let Some((line, attribute)) = reader
                && group.delimiter() == Delimiter::Parenthesis
                && let Some((target, checked)) = read_in(group.stream(), attribute)
            {
                reads.push(Read {
                    target,
                    line,
                    checked,
                });
            }
            collect(group.stream(), reads);
        }
        reader = match &token {
            TokenTree::Ident(word)
                if READERS.iter().any(|reader| word == reader)
                    && !is_punct(previous.as_ref(), '.') =>
            {
                Some((word.span().start().line, word == "starmap"))
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

/// Returns what the parentheses after a reader read: the name of a string
/// literal, or, outside a field attribute, the target of a `format!` call.
/// The flag says whether a field attribute names a check.
fn read_in(tokens: TokenStream, attribute: bool) -> Option<(Target, bool)> {
    if let Some((name, checked)) = literal_name(tokens.clone(), attribute) {
        return Some((Target::Variable(name), checked));
    }
    if attribute {
        return None;
    }
    formatted(tokens).map(|target| (target, false))
}

/// Returns the target of a read when `tokens` are one `format!` call, with
/// or without a leading `&`, a path such as `std::format!` and a trailing
/// comma, whose first argument is a string literal.
fn formatted(tokens: TokenStream) -> Option<Target> {
    let mut tokens = tokens.into_iter().peekable();
    if is_punct(tokens.peek(), '&') {
        tokens.next();
    }
    let mut last = None;
    loop {
        match tokens.next()? {
            TokenTree::Ident(word) => last = Some(word),
            TokenTree::Punct(punct) if punct.as_char() == ':' => {}
            TokenTree::Punct(punct) if punct.as_char() == '!' => break,
            _ => return None,
        }
    }
    if !last.is_some_and(|word| word == "format") {
        return None;
    }
    let Some(TokenTree::Group(arguments)) = tokens.next() else {
        return None;
    };
    match (tokens.next(), tokens.next()) {
        (None, _) => {}
        (Some(TokenTree::Punct(comma)), None) if comma.as_char() == ',' => {}
        _ => return None,
    }
    let Some(TokenTree::Literal(literal)) = arguments.stream().into_iter().next() else {
        return None;
    };
    let text = string_text(&literal.to_string())?.to_owned();
    Some(target_of(&text))
}

/// Returns what a `format!` literal whose text is `text` reads: a family
/// when it is a name, an underscore and one trailing placeholder, the name
/// itself when it has no placeholder, and nothing the scan can resolve
/// otherwise.
fn target_of(text: &str) -> Target {
    let Some((literal, placeholders)) = pattern(text) else {
        return Target::Unresolved;
    };
    let resolved = match placeholders.as_slice() {
        [] => Name::from_str(&literal).ok().map(Target::Variable),
        [at] if *at == literal.len() => literal
            .strip_suffix('_')
            .and_then(|prefix| Name::from_str(prefix).ok())
            .map(Target::Family),
        _ => None,
    };
    resolved.unwrap_or(Target::Unresolved)
}

/// Splits a `format!` literal's `text` as `format!` reads it: the literal
/// text, with `{{` and `}}` as single braces, and the byte offset in that
/// text of each placeholder. Returns `None` for a lone `}` or an unclosed
/// `{`, which `format!` refuses.
fn pattern(text: &str) -> Option<(String, Vec<usize>)> {
    let mut literal = String::new();
    let mut placeholders = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                literal.push('{');
            }
            '{' => {
                chars.by_ref().find(|&inner| inner == '}')?;
                placeholders.push(literal.len());
            }
            '}' if chars.next_if_eq(&'}').is_some() => literal.push('}'),
            '}' => return None,
            other => literal.push(other),
        }
    }
    Some((literal, placeholders))
}

/// Returns the text between the quotes of a string literal as Rust writes
/// it, `"..."` or a raw string's `r#"..."#`, or `None` for any other
/// literal. Escapes stay as written, so a text with one is no name.
fn string_text(written: &str) -> Option<&str> {
    let text = written
        .strip_prefix('r')
        .unwrap_or(written)
        .trim_matches('#');
    text.strip_prefix('"')?.strip_suffix('"')
}

/// Returns the variable name that `tokens` hold when they are one string
/// literal, with or without a trailing comma, whose text is a valid name.
/// When `attribute` is set, the tokens are a field attribute's, and the
/// literal may be followed by `, check = <path>`; the returned flag says
/// whether it is.
fn literal_name(tokens: TokenStream, attribute: bool) -> Option<(Name, bool)> {
    let mut tokens = tokens.into_iter();
    let Some(TokenTree::Literal(literal)) = tokens.next() else {
        return None;
    };
    let checked = match (tokens.next(), tokens.next()) {
        (None, _) => false,
        (Some(TokenTree::Punct(comma)), None) if comma.as_char() == ',' => false,
        (Some(TokenTree::Punct(comma)), Some(TokenTree::Ident(key)))
            if attribute && comma.as_char() == ',' && key == "check" =>
        {
            true
        }
        _ => return None,
    };
    // A name holds no quote and no backslash, so its literal is the name
    // between quotes, or between a raw string's `r`, hashes and quotes.
    let name = string_text(&literal.to_string())?.to_owned();
    Name::from_str(&name).ok().map(|name| (name, checked))
}

/// A source file or directory that the scan cannot read.
#[derive(Debug, Failure)]
pub(crate) enum SourceFailure {
    /// The path cannot be read, or is not UTF-8 text or valid Rust.
    #[failure(
        code = SOURCE_UNREADABLE,
        hint = "fix {file} so that cargo builds it",
        alternative = Recovery::Hint(format!("move {file} out of src")),
        field(file = file.as_str()),
    )]
    Unbuildable {
        /// The path, as the scan found it.
        file: String,
        /// What is wrong with it, such as `is not valid Rust at line 3`.
        problem: String,
    },
    /// The file is over 1 MiB. Such a file is usually generated and
    /// builds, so its one repair is to move it.
    #[failure(
        code = SOURCE_UNREADABLE,
        hint = "move generated code out of src",
        field(file = file.as_str()),
    )]
    TooLarge {
        /// The path, as the scan found it.
        file: String,
    },
}

impl SourceFailure {
    /// Returns the failure of the file at `path`, which `problem` states.
    fn new(path: &Path, problem: String) -> Self {
        Self::Unbuildable {
            file: path.display().to_string(),
            problem,
        }
    }

    /// Returns the failure of the file at `path`, which is over 1 MiB.
    fn too_large(path: &Path) -> Self {
        Self::TooLarge {
            file: path.display().to_string(),
        }
    }
}

impl fmt::Display for SourceFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unbuildable { file, problem } => write!(formatter, "{file} {problem}"),
            Self::TooLarge { file } => {
                write!(
                    formatter,
                    "{file} is over 1 MiB, so the scan does not read it"
                )
            }
        }
    }
}

impl std::error::Error for SourceFailure {}
