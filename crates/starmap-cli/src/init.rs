//! `starmap init`, which writes a spec with a guess for each variable that
//! the application's source reads.

use std::fmt;
use std::io;
use std::path::Path;

use crumbkit::schemars::{Schema, SchemaGenerator};
use crumbkit::serde::Serializer;
use crumbkit::{Document, Listing, Row, Shown};
use schemars::JsonSchema;
use serde::Serialize;
use starmap::Name;

use crate::Failed;
use crate::cli::{InitArgs, SpecFile};
use crate::files;
use crate::scan;
use crate::spec::SpecFailure;

#[cfg(test)]
mod tests;

/// The document that `starmap init` prints.
pub(crate) const INIT: Document<Listing<Guess>> = Document::new(
    "starmap",
    "init",
    1,
    "Each variable that src reads, with the guesses that init writes to the spec.",
);

/// The variables that the shell and the XDG base directories set, which
/// init guesses are inherited. Every name that starts with `XDG_` is
/// inherited too.
const INHERITED: &[&str] = &["HOME", "PATH", "LANG", "TZ"];

/// The words of a name, between underscores, that make init guess a
/// withheld secret.
const SECRET_WORDS: &[&str] = &["TOKEN", "SECRET", "KEY", "PASSWORD"];

/// The endings of a name that make init guess a type, in the order init
/// tries them.
const FORM_ENDINGS: &[(&str, &str)] = &[
    ("_PORT", "port"),
    ("_URL", "url"),
    ("_DIR", "path"),
    ("_PATH", "path"),
    ("_FILE", "path"),
    ("_HOME", "path"),
];

/// The first lines of the file that init writes.
const HEADER: &str = "\
# starmap init wrote this file from the variables that the source reads.
# Each value that ends in ? is a guess. Confirm or correct each one, delete
# its ?, and write each purpose. starmap check refuses the file until then.
";

/// One variable that src reads, with the guess for each field that init
/// writes. Each guess ends in `?`, as it does in the file.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Guess {
    /// The variable's name.
    name: String,
    /// The guessed type, such as port?.
    #[serde(rename = "type")]
    form: String,
    /// The guessed unset rule: required? for a setting, and optional? otherwise.
    unset: String,
    /// The guessed show: withheld? for a secret, and shown? otherwise.
    show: String,
    /// The guessed source: setting?, secret? or inherited?.
    source: String,
    /// The places in src that read the variable, such as src/serve.rs:22.
    read_at: Places,
}

impl Row for Guess {
    const COLUMNS: &'static [&'static str] = &["name", "type", "unset", "source", "read_at"];
    const LONG_COLUMNS: &'static [&'static str] = &["show"];
    const UNCUT_COLUMNS: &'static [&'static str] = &["name"];
}

/// The places that read a variable. The text separates them with spaces,
/// and the JSON is a list.
#[derive(Debug)]
pub(crate) struct Places(Vec<String>);

impl fmt::Display for Places {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0.join(" "))
    }
}

impl Shown for Places {
    fn json<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }

    fn schema(generator: &mut SchemaGenerator) -> Schema {
        Vec::<String>::json_schema(generator)
    }
}

crumbkit::shown!(Places);

/// Scans the src directory beside the spec path that `--spec` names, and
/// writes a spec there with a guess for each variable that src reads. A
/// dry run prints the same guesses and writes nothing.
///
/// # Errors
///
/// Returns [`Failed`] when a file is at the spec path already, when the
/// spec cannot be written, and when src cannot be scanned.
pub(crate) fn init(args: &InitArgs, dry_run: bool) -> Result<Listing<Guess>, Failed> {
    let path = &args.spec;
    let file = path.display().to_string();
    if files::exists(path) {
        return Err(SpecFailure::Exists { file }.into());
    }
    let src = path.parent().unwrap_or(Path::new("")).join("src");
    let guesses: Vec<Guess> = scan::scan(&src)?
        .into_iter()
        .map(|(name, places)| guess(name, places))
        .collect();
    let count = guesses.len();
    let noun = if count == 1 { "variable" } else { "variables" };
    let src = src.display();
    let listing = if dry_run {
        Listing::new(guesses, count)
            .note(format!(
                "{src} reads {count} {noun}, and init wrote nothing."
            ))
            .next(SpecFile::call(&["init"], &file), "writes the file.")
    } else {
        files::write_new(path, &spec_text(&guesses)).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                SpecFailure::Exists { file: file.clone() }
            } else {
                SpecFailure::Unwritable {
                    file: file.clone(),
                    error,
                }
            }
        })?;
        Listing::new(guesses, count)
            .note(format!(
                "Wrote {file} with {count} {noun} that {src} reads. Each value marked ? is a guess."
            ))
            .next(
                SpecFile::call(&["check"], &file),
                "refuses the file until each guess is confirmed and its ? deleted.",
            )
    };
    Ok(listing.described(noun))
}

/// Returns the guesses for the variable `name`, which src reads at
/// `places`. The name decides each guess, as the build plan's rules say.
fn guess(name: Name, places: Vec<String>) -> Guess {
    let text = name.as_str();
    let inherited = INHERITED.contains(&text) || text.starts_with("XDG_");
    let secret = !inherited && text.split('_').any(|word| SECRET_WORDS.contains(&word));
    let form = FORM_ENDINGS
        .iter()
        .find(|(ending, _)| !secret && text.ends_with(ending))
        .map_or("string", |(_, form)| form);
    let (unset, show, source) = if secret {
        ("optional", "withheld", "secret")
    } else if inherited {
        ("optional", "shown", "inherited")
    } else {
        ("required", "shown", "setting")
    };
    Guess {
        name: text.to_owned(),
        form: format!("{form}?"),
        unset: format!("{unset}?"),
        show: format!("{show}?"),
        source: format!("{source}?"),
        read_at: Places(places),
    }
}

/// Returns the text of the spec that holds `guesses`: one table for each
/// variable, with a comment that names the places that read it and a
/// purpose of `?`.
fn spec_text(guesses: &[Guess]) -> String {
    let mut text = HEADER.to_owned();
    for guess in guesses {
        // A name holds only upper case letters, digits and underscores, so
        // it needs no quotes. A file name may hold a line break, so the
        // comment escapes it.
        let places = guess.read_at.0.join(", ");
        text.push_str(&format!(
            "\n[variables.{}]\n# Read at {}.\npurpose = \"?\"\ntype = \"{}\"\nunset = \"{}\"\nshow = \"{}\"\nsource = \"{}\"\n",
            guess.name,
            places.escape_debug(),
            guess.form,
            guess.unset,
            guess.show,
            guess.source,
        ));
    }
    text
}
