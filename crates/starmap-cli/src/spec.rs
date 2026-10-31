//! Reading `starmap.toml` into a [`Spec`], and the failures of a file that
//! cannot be read or parsed.

use std::fmt;
use std::io;
use std::path::Path;

use crumbkit::{Code, Failure, Field, FieldValue, Recovery};
use miette::{Diagnostic, LabeledSpan, NamedSource, SourceCode};
use starmap::{Spec, SpecError, SpecProblemKind};

use crate::cli::SpecFile;
use crate::codes;
use crate::files;

#[cfg(test)]
mod tests;

/// Reads and parses the spec at `path`. The read stops one byte past
/// [`Spec::MAX_BYTES`], so a larger file costs no more memory and fails as
/// too large without being parsed (BASE-34).
///
/// # Errors
///
/// Returns [`SpecFailure`] when the file is missing, cannot be read, is over
/// 256 KiB, is not UTF-8 text, or does not parse.
pub(crate) fn load(path: &Path) -> Result<Spec, SpecFailure> {
    let file = path.display().to_string();
    let bytes = files::read_capped(path, Spec::MAX_BYTES + 1).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            SpecFailure::NotFound { file: file.clone() }
        } else {
            SpecFailure::Unreadable {
                file: file.clone(),
                error,
            }
        }
    })?;
    if bytes.len() > Spec::MAX_BYTES {
        return Err(SpecFailure::TooLarge { file });
    }
    let text = String::from_utf8(bytes).map_err(|error| SpecFailure::NotUtf8 {
        file: file.clone(),
        at: error.utf8_error().valid_up_to(),
    })?;
    Spec::parse(&text).map_err(|error| SpecFailure::Invalid {
        source: NamedSource::new(file.clone(), text),
        file,
        error,
    })
}

/// A spec file that cannot be read or parsed.
#[derive(Debug)]
pub(crate) enum SpecFailure {
    /// No file is at the path.
    NotFound {
        /// The path, as the caller gave it.
        file: String,
    },
    /// `init` finds a file at the path already, and keeps it.
    Exists {
        /// The path, as the caller gave it.
        file: String,
    },
    /// `init` cannot create or write the file.
    Unwritable {
        /// The path, as the caller gave it.
        file: String,
        /// Why it cannot be written.
        error: io::Error,
    },
    /// The file exists and cannot be read.
    Unreadable {
        /// The path, as the caller gave it.
        file: String,
        /// Why it cannot be read.
        error: io::Error,
    },
    /// The file is over [`Spec::MAX_BYTES`], so it is not parsed.
    TooLarge {
        /// The path, as the caller gave it.
        file: String,
    },
    /// The file is not UTF-8 text, which TOML requires.
    NotUtf8 {
        /// The path, as the caller gave it.
        file: String,
        /// The offset of the first byte that is not UTF-8.
        at: usize,
    },
    /// The file does not parse, for at least one problem.
    Invalid {
        /// The path, as the caller gave it.
        file: String,
        /// The file's text, which each problem's label points into.
        source: NamedSource<String>,
        /// Every problem that the parser found.
        error: SpecError,
    },
}

impl SpecFailure {
    /// Returns the kind of the first problem of a file that does not
    /// parse.
    fn first_kind(&self) -> Option<SpecProblemKind> {
        match self {
            Self::Invalid { error, .. } => error.problems().first().map(|problem| problem.kind()),
            Self::NotFound { .. }
            | Self::Exists { .. }
            | Self::Unwritable { .. }
            | Self::Unreadable { .. }
            | Self::TooLarge { .. }
            | Self::NotUtf8 { .. } => None,
        }
    }

    /// Returns how many problems a file that does not parse has when it
    /// has several and each one is a guess that `starmap init` wrote.
    fn guesses(&self) -> Option<usize> {
        let Self::Invalid { error, .. } = self else {
            return None;
        };
        let problems = error.problems();
        let all_guesses = problems
            .iter()
            .all(|problem| problem.kind() == SpecProblemKind::Guess);
        (problems.len() > 1 && all_guesses).then_some(problems.len())
    }
}

/// Prints what failed. A file with one problem prints that problem's
/// message. A file with several guesses and no other problem prints how
/// many guesses it holds, and a file with several problems prints how many
/// it has, such as `starmap.toml has 2 problems`, and the labels describe
/// each one.
impl fmt::Display for SpecFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { file } => write!(formatter, "{file} does not exist"),
            Self::Exists { file } => write!(formatter, "{file} exists already"),
            Self::Unwritable { file, .. } => write!(formatter, "{file} cannot be written"),
            Self::Unreadable { file, .. } => write!(formatter, "{file} cannot be read"),
            Self::TooLarge { file } => write!(formatter, "{file} is over 256 KiB"),
            Self::NotUtf8 { file, at } => write!(
                formatter,
                "{file} is not UTF-8 text at byte {at}, so it is not TOML"
            ),
            Self::Invalid { file, error, .. } => match (error.problems(), self.guesses()) {
                ([problem], _) => write!(formatter, "{file}: {problem}"),
                (_, Some(count)) => write!(
                    formatter,
                    "{file} still holds {count} guesses that starmap init marked with ?"
                ),
                (problems, None) => write!(formatter, "{file} has {} problems", problems.len()),
            },
        }
    }
}

/// A file that cannot be read has the I/O error as its source, which
/// prints as the failure's cause.
impl std::error::Error for SpecFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable { error, .. } | Self::Unwritable { error, .. } => Some(error),
            Self::NotFound { .. }
            | Self::Exists { .. }
            | Self::TooLarge { .. }
            | Self::NotUtf8 { .. }
            | Self::Invalid { .. } => None,
        }
    }
}

/// Each problem of a file that does not parse is a label at its span, so the
/// output shows every problem with the lines it is about. The label of a
/// file's only problem is `here`, because the failure's message already
/// states the problem. A file whose problems are all guesses has one label,
/// at the first guess, because each guess has the same fix. The label of
/// each of several other problems is its message. The `problems` field
/// lists the codes of every problem.
impl Diagnostic for SpecFailure {
    fn source_code(&self) -> Option<&dyn SourceCode> {
        match self {
            Self::Invalid { source, .. } => Some(source),
            Self::NotFound { .. }
            | Self::Exists { .. }
            | Self::Unwritable { .. }
            | Self::Unreadable { .. }
            | Self::TooLarge { .. }
            | Self::NotUtf8 { .. } => None,
        }
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let Self::Invalid { error, .. } = self else {
            return None;
        };
        if self.guesses().is_some() {
            let span = error.problems().first()?.span();
            let label =
                LabeledSpan::new(Some("the first guess".to_owned()), span.start, span.len());
            return Some(Box::new(std::iter::once(label)));
        }
        let only = error.problems().len() == 1;
        Some(Box::new(error.problems().iter().map(move |problem| {
            let span = problem.span();
            let text = if only {
                "here".to_owned()
            } else {
                problem.to_string()
            };
            LabeledSpan::new(Some(text), span.start, span.len())
        })))
    }
}

impl Failure for SpecFailure {
    fn code(&self) -> &'static Code {
        match self {
            Self::NotFound { .. } => &codes::SPEC_NOT_FOUND,
            Self::Exists { .. } => &codes::SPEC_EXISTS,
            Self::Unwritable { .. } => &codes::SPEC_UNWRITABLE,
            Self::Unreadable { .. } => &codes::SPEC_UNREADABLE,
            Self::TooLarge { .. } => &codes::SPEC_TOO_LARGE,
            Self::NotUtf8 { .. } => &codes::SPEC_SYNTAX,
            Self::Invalid { .. } => self.first_kind().map_or(&codes::SPEC_SYNTAX, code),
        }
    }

    fn recovery(&self) -> Recovery {
        let hint = match self {
            Self::NotFound { file } => return Recovery::Run(SpecFile::call(&["init"], file)),
            Self::Exists { file } => return Recovery::Run(SpecFile::call(&["check"], file)),
            Self::Unwritable { file, .. } => {
                format!("make the directory of {file} writable, or pass --spec another file")
            }
            Self::Unreadable { file, .. } => {
                format!("make {file} readable, or pass --spec another file")
            }
            Self::TooLarge { .. } => hint(SpecProblemKind::TooLarge).to_owned(),
            Self::NotUtf8 { file, .. } => format!("save {file} as UTF-8 text"),
            Self::Invalid { .. } => self.first_kind().map_or("", hint).to_owned(),
        };
        Recovery::Hint(hint)
    }

    fn fields(&self) -> Vec<Field<'_>> {
        match self {
            Self::NotFound { file }
            | Self::Exists { file }
            | Self::Unwritable { file, .. }
            | Self::Unreadable { file, .. }
            | Self::TooLarge { file }
            | Self::NotUtf8 { file, .. } => {
                vec![Field::new("file", file)]
            }
            Self::Invalid { file, error, .. } => vec![
                Field::new("file", file),
                Field::new(
                    "problems",
                    FieldValue::list(error.problems().iter().map(|problem| problem.kind().code())),
                ),
            ],
        }
    }

    fn diagnostic(&self) -> Option<&dyn Diagnostic> {
        Some(self)
    }
}

/// Returns the code of a spec problem of `kind`. A kind that a later
/// release of starmap adds reports as a syntax problem until this list
/// names it.
fn code(kind: SpecProblemKind) -> &'static Code {
    match kind {
        SpecProblemKind::TooLarge => &codes::SPEC_TOO_LARGE,
        SpecProblemKind::UnknownField => &codes::SPEC_UNKNOWN_FIELD,
        SpecProblemKind::MissingField => &codes::SPEC_MISSING_FIELD,
        SpecProblemKind::BadValue => &codes::SPEC_BAD_VALUE,
        SpecProblemKind::BadName => &codes::SPEC_BAD_NAME,
        SpecProblemKind::Guess => &codes::SPEC_GUESS,
        SpecProblemKind::BadDefault => &codes::SPEC_BAD_DEFAULT,
        SpecProblemKind::BadRange => &codes::SPEC_BAD_RANGE,
        SpecProblemKind::EmptyEnumeration => &codes::SPEC_EMPTY_ENUMERATION,
        SpecProblemKind::ShownSecret => &codes::SPEC_SHOWN_SECRET,
        SpecProblemKind::SecretForm => &codes::SPEC_SECRET_FORM,
        SpecProblemKind::BindMismatch => &codes::SPEC_BIND_MISMATCH,
        SpecProblemKind::BadBytes => &codes::SPEC_BAD_BYTES,
        SpecProblemKind::UnknownPlaceholder => &codes::SPEC_UNKNOWN_PLACEHOLDER,
        SpecProblemKind::ModeUndeclared => &codes::SPEC_MODE_UNDECLARED,
        SpecProblemKind::ModeSource => &codes::SPEC_MODE_SOURCE,
        // `Syntax`, and any kind that a later release adds.
        _ => &codes::SPEC_SYNTAX,
    }
}

/// Returns the next step for a spec problem of `kind`.
fn hint(kind: SpecProblemKind) -> &'static str {
    match kind {
        SpecProblemKind::TooLarge => {
            "remove the variables the application does not read, so the file is at most 256 KiB"
        }
        SpecProblemKind::UnknownField => "use one of the fields the failure lists",
        SpecProblemKind::MissingField => "add the field the failure names",
        SpecProblemKind::BadValue => "set a value the field takes, as the failure lists",
        SpecProblemKind::BadName => {
            "rename the variable with upper case letters, digits and underscores"
        }
        SpecProblemKind::Guess => "confirm or correct each guess and delete its ?",
        SpecProblemKind::BadDefault => "set a default the type accepts",
        SpecProblemKind::BadRange => "swap or correct the bounds",
        SpecProblemKind::EmptyEnumeration => "list each value once",
        SpecProblemKind::ShownSecret => "set show = \"withheld\"",
        SpecProblemKind::SecretForm => "set type = \"string\"",
        SpecProblemKind::BindMismatch => "change the source or the bind, as the failure lists",
        SpecProblemKind::BadBytes => "set bytes from 16 to 1024 on a generated variable",
        SpecProblemKind::UnknownPlaceholder => "use {workspace}, {checkout} or {home}",
        SpecProblemKind::ModeUndeclared => "declare the mode variable",
        SpecProblemKind::ModeSource => "make the mode variable a setting",
        // `Syntax`, and any kind that a later release adds.
        _ => "fix the line the failure points at",
    }
}
