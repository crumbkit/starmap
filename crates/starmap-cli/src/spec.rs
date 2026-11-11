//! Reading `starmap.toml` into a [`Spec`], following the file it points
//! to when it is a [`Pointer`], and the failures of a file that cannot be
//! read or parsed.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crumbkit::{Code, Failure, Field, FieldValue, Recovery, Spelling};
use miette::{Diagnostic, LabeledSpan, NamedSource, SourceCode};
use starmap::{Pointer, Spec, SpecError, SpecProblemKind};

use crate::cli::SpecFile;
use crate::codes;
use crate::files;

#[cfg(test)]
mod tests;

/// The spec that `--spec` names, read through its pointer when it holds
/// one, with the source directories and the `.env` file that go with it.
#[derive(Debug)]
pub(crate) struct Opened {
    /// The parsed spec.
    pub(crate) spec: Spec,
    /// Where the spec's declarations and the files beside them are.
    pub(crate) located: Located,
}

/// Where the declarations of the file that `--spec` names are. A file that
/// declares variables is its own declaration file, with `src` and `.env`
/// beside it. A pointer names the declaration file, the source directories
/// and the `.env` file, each relative to the pointer's directory.
#[derive(Debug)]
pub(crate) struct Located {
    /// The file that declares the variables.
    pub(crate) file: PathBuf,
    /// The directories whose source a scan reads.
    pub(crate) src: Vec<PathBuf>,
    /// The `.env` file that `run` reads.
    pub(crate) dotenv: PathBuf,
    /// The pointer file, as the caller gave it, when `--spec` names a
    /// pointer.
    pub(crate) pointer: Option<String>,
    /// The text of the file, already read, when `--spec` names a file that
    /// declares variables.
    text: Option<String>,
}

impl Located {
    /// Returns the place of a spec that declares its variables at `path`,
    /// whose text `text` is when it was read.
    fn beside(path: &Path, text: Option<String>) -> Self {
        let dir = path.parent().unwrap_or(Path::new(""));
        Self {
            file: path.to_path_buf(),
            src: vec![dir.join(Pointer::SRC)],
            dotenv: dir.join(Pointer::DOTENV),
            pointer: None,
            text,
        }
    }
}

/// Reads and parses the spec that `path` names. When the file is a
/// pointer, it reads the file that the pointer names, and that file must
/// declare the variables.
///
/// # Errors
///
/// Returns [`SpecFailure`] as [`locate`] does, and when the declaration
/// file is missing, cannot be read, is over 256 KiB, is not UTF-8 text, or
/// does not parse.
pub(crate) fn open(path: &Path) -> Result<Opened, SpecFailure> {
    let mut located = locate(path)?;
    let file = located.file.display().to_string();
    let spec = match located.text.take() {
        Some(text) => parse_text(file, text),
        None => read(&located.file).and_then(|bytes| parse(file, bytes)),
    }
    .map_err(|failure| failure.through(located.pointer.as_deref()))?;
    Ok(Opened { spec, located })
}

/// Reads the file at `path` and returns where its declarations are. A
/// missing file declares nothing yet, so it is its own declaration file,
/// which `init` writes and every other command reports as missing.
///
/// # Errors
///
/// Returns [`SpecFailure`] when the file cannot be read, is over 256 KiB,
/// is not UTF-8 text, or is a pointer that does not parse.
pub(crate) fn locate(path: &Path) -> Result<Located, SpecFailure> {
    let file = path.display().to_string();
    let bytes = match read(path) {
        Ok(bytes) => bytes,
        Err(SpecFailure::NotFound { .. }) => return Ok(Located::beside(path, None)),
        Err(failure) => return Err(failure),
    };
    let text = utf8(&file, bytes)?;
    match Pointer::parse(&text) {
        Ok(None) => Ok(Located::beside(path, Some(text))),
        Ok(Some(pointer)) => {
            let dir = path.parent().unwrap_or(Path::new(""));
            Ok(Located {
                file: dir.join(pointer.spec()),
                src: pointer.src().iter().map(|src| dir.join(src)).collect(),
                dotenv: dir.join(pointer.dotenv()),
                pointer: Some(file),
                text: None,
            })
        }
        Err(error) => Err(SpecFailure::Invalid {
            source: NamedSource::new(file.clone(), text),
            file,
            error,
        }),
    }
}

/// Reads the file at `path`. The read stops one byte past
/// [`Spec::MAX_BYTES`], so a larger file costs no more memory and fails as
/// too large without being parsed (BASE-34).
fn read(path: &Path) -> Result<Vec<u8>, SpecFailure> {
    let file = path.display().to_string();
    let bytes = files::read_capped(path, Spec::MAX_BYTES + 1).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            SpecFailure::NotFound {
                file: file.clone(),
                pointer: None,
            }
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
    Ok(bytes)
}

/// Returns `bytes`, the text of the file that `file` names, as text.
fn utf8(file: &str, bytes: Vec<u8>) -> Result<String, SpecFailure> {
    String::from_utf8(bytes).map_err(|error| SpecFailure::NotUtf8 {
        file: file.to_owned(),
        at: error.utf8_error().valid_up_to(),
    })
}

/// Parses `bytes`, the text of the spec that `file` names, which is at
/// most [`Spec::MAX_BYTES`].
///
/// # Errors
///
/// Returns [`SpecFailure`] when the bytes are not UTF-8 text or do not
/// parse.
pub(crate) fn parse(file: String, bytes: Vec<u8>) -> Result<Spec, SpecFailure> {
    let text = utf8(&file, bytes)?;
    parse_text(file, text)
}

/// Parses `text`, the text of the spec that `file` names.
fn parse_text(file: String, text: String) -> Result<Spec, SpecFailure> {
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
        /// The path, as the caller gave it or as a pointer names it.
        file: String,
        /// The pointer that names the path, as the caller gave it.
        pointer: Option<String>,
    },
    /// `init` finds a file at the path already, and keeps it.
    Exists {
        /// The path, as the caller gave it or as a pointer names it.
        file: String,
        /// The pointer that names the path, as the caller gave it.
        pointer: Option<String>,
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
    /// Returns the failure of a declaration file that `pointer` names, so
    /// that a missing file names the pointer and its repair runs on the
    /// pointer.
    fn through(self, pointer: Option<&str>) -> Self {
        match (self, pointer) {
            (Self::NotFound { file, .. }, Some(pointer)) => Self::NotFound {
                file,
                pointer: Some(pointer.to_owned()),
            },
            (failure, _) => failure,
        }
    }

    /// Returns the file that a repair passes to `--spec`: the pointer when
    /// there is one, and otherwise the file.
    fn given<'a>(file: &'a str, pointer: Option<&'a String>) -> &'a str {
        pointer.map_or(file, String::as_str)
    }

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
            Self::NotFound {
                file,
                pointer: None,
            } => write!(formatter, "{file} does not exist"),
            Self::NotFound {
                file,
                pointer: Some(pointer),
            } => write!(
                formatter,
                "{file}, which {pointer} points to, does not exist"
            ),
            Self::Exists {
                file,
                pointer: None,
            } => write!(formatter, "{file} exists already"),
            Self::Exists {
                file,
                pointer: Some(pointer),
            } => write!(
                formatter,
                "{file}, which {pointer} points to, exists already"
            ),
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
            // A withheld default is in the text, so the excerpt would print
            // the secret.
            Self::Invalid { error, .. }
                if error
                    .problems()
                    .iter()
                    .any(|problem| problem.kind() == SpecProblemKind::WithheldDefault) =>
            {
                None
            }
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
            Self::NotFound { file, pointer } => {
                return Recovery::Run(SpecFile::call(
                    &["init"],
                    Self::given(file, pointer.as_ref()),
                ));
            }
            Self::Exists { file, pointer } => {
                return Recovery::Run(SpecFile::call(
                    &["check"],
                    Self::given(file, pointer.as_ref()),
                ));
            }
            Self::Unwritable { file, .. } => format!("make the directory of {file} writable"),
            Self::Unreadable { file, .. } => format!("make {file} readable"),
            Self::TooLarge { .. } => hint(SpecProblemKind::TooLarge).to_owned(),
            Self::NotUtf8 { file, .. } => format!("save {file} as UTF-8 text"),
            Self::Invalid { .. } => self.first_kind().map_or("", hint).to_owned(),
        };
        Recovery::Hint(hint)
    }

    /// Returns the second repair of a spec file that cannot be written or
    /// read, which is to pass another file with `--spec`.
    fn alternatives(&self) -> Vec<Recovery> {
        match self {
            Self::Unwritable { .. } | Self::Unreadable { .. } => {
                vec![Recovery::hint("pass --spec another file")]
            }
            Self::NotFound { .. }
            | Self::Exists { .. }
            | Self::TooLarge { .. }
            | Self::NotUtf8 { .. }
            | Self::Invalid { .. } => Vec::new(),
        }
    }

    fn fields(&self) -> Vec<Field<'_>> {
        match self {
            Self::NotFound { file, pointer } | Self::Exists { file, pointer } => {
                let mut fields = vec![Field::new("file", file)];
                if let Some(pointer) = pointer {
                    fields.push(Field::new("pointer", pointer));
                }
                fields
            }
            Self::Unwritable { file, .. }
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

/// Returns the code of a spec problem of `kind`: the code in
/// [`codes::ALL`] that spells as [`SpecProblemKind::code`]. A kind that a
/// later release of starmap adds reports as a syntax problem until
/// `codes` declares its code.
fn code(kind: SpecProblemKind) -> &'static Code {
    codes::ALL
        .iter()
        .copied()
        .find(|code| Spelling::COLONS.spell(code) == kind.code())
        .unwrap_or(&codes::SPEC_SYNTAX)
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
        SpecProblemKind::BadPattern => {
            "write a pattern of at most 1024 bytes in the syntax of the Rust regex crate"
        }
        SpecProblemKind::EmptyEnumeration => "list at least one value, and each value once",
        SpecProblemKind::ShownSecret => "set show = \"withheld\"",
        SpecProblemKind::WithheldType => "set type = \"string\"",
        SpecProblemKind::WithheldDefault => {
            "remove the default: set a secret to unset = \"required\" with a 1Password bind or no bind so that starmap run asks for it, drop it from a generated variable because starmap creates the value, and give a withheld setting its value in the shell"
        }
        SpecProblemKind::BindMismatch => "change the source or the bind, as the failure lists",
        SpecProblemKind::BadBytes => {
            "set bytes from 16 to 1024 on a generated string variable, give a bytes type a length from 1 to 1024, and give a generated variable's bytes type a length from 16 to 1024 and leave out bytes"
        }
        SpecProblemKind::UnknownPlaceholder => "use {workspace}, {checkout} or {home}",
        SpecProblemKind::ModeUndeclared => "declare the mode variable",
        SpecProblemKind::ModeSource => "make the mode variable a setting",
        SpecProblemKind::PointerDeclares => {
            "move the declarations to the file that spec names, or remove spec, src and dotenv"
        }
        SpecProblemKind::PointerChain => "point spec at the file that declares the variables",
        SpecProblemKind::EnumerationUndeclared => {
            "declare the enumeration in an [enumerations.<name>] table, or correct the name"
        }
        SpecProblemKind::GroupUndeclared => {
            "declare the variable, or remove it from the group's variables"
        }
        SpecProblemKind::BadGroup => {
            "list at least two variables in each group, each once and in one group only, and none with a default or a generated value"
        }
        // `Syntax`, and any kind that a later release adds.
        _ => "fix the line the failure points at",
    }
}
