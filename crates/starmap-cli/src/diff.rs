//! `starmap diff`, which compares the spec in the working tree with the
//! spec that a git revision holds, and lists each variable that was added,
//! changed or removed.

use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::path::Path;

use crumbkit::{Batch, Code, Document, Failure, Field, Recovery, Row};
use schemars::JsonSchema;
use serde::Serialize;
use starmap::{Form, Name, Source, Spec, Variable};

use crate::Failed;
use crate::cli::DiffArgs;
use crate::codes;
use crate::explain::{bind_text, form_text, source_text, unset_text};
use crate::git::{self, Entry, GitError};
use crate::spec::{self, SpecFailure};

#[cfg(test)]
mod tests;

/// The document that `starmap diff` prints.
pub(crate) const DIFF: Document<Batch<Change, crumbkit::Error>> = Document::new(
    "starmap",
    "diff",
    1,
    "Each variable that was added, changed or removed between a git revision's spec and the spec in the working tree.",
);

/// One difference between the two specs: a variable that was added or
/// removed, or one field of a variable that changed.
#[derive(Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub(crate) struct Change {
    /// Whether the variable was added, changed or removed.
    change: Kind,
    /// The variable's name.
    name: String,
    /// The field that changed, such as type or unset; null for a variable that was added or removed.
    field: Option<String>,
    /// The field's value at the revision, or the type, show and source of a removed variable; null for an added variable.
    was: Option<String>,
    /// The field's value in the working tree, or the type, show and source of an added variable; null for a removed variable.
    now: Option<String>,
}

impl Row for Change {
    const COLUMNS: &'static [&'static str] = &["change", "name", "field", "was", "now"];
    const UNCUT_COLUMNS: &'static [&'static str] = &["name"];
}

/// How a variable differs between the two specs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Kind {
    /// The working tree declares the variable and the revision does not.
    Added,
    /// Both declare the variable, and this field differs.
    Changed,
    /// The revision declares the variable and the working tree does not.
    Removed,
}

/// A revision that `starmap diff` cannot read.
#[derive(Debug)]
pub(crate) enum RevisionFailure {
    /// The spec's directory is not in a git repository.
    NoRepository {
        /// The spec's directory, or an empty string for the current
        /// directory.
        dir: String,
    },
    /// The revision names no commit.
    NotFound {
        /// The revision, as the caller gave it.
        revision: String,
    },
    /// git cannot start or cannot read the revision.
    Unreadable {
        /// The revision, as the caller gave it.
        revision: String,
        /// Why git cannot read it.
        error: io::Error,
    },
}

impl fmt::Display for RevisionFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRepository { dir } if dir.is_empty() || dir == "." => {
                formatter.write_str("the current directory is not in a git repository")
            }
            Self::NoRepository { dir } => write!(formatter, "{dir} is not in a git repository"),
            Self::NotFound { revision } => {
                write!(formatter, "{revision} names no revision in this repository")
            }
            Self::Unreadable { revision, .. } => write!(formatter, "git cannot read {revision}"),
        }
    }
}

impl std::error::Error for RevisionFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable { error, .. } => Some(error),
            Self::NoRepository { .. } | Self::NotFound { .. } => None,
        }
    }
}

impl Failure for RevisionFailure {
    fn code(&self) -> &'static Code {
        match self {
            Self::NoRepository { .. } => &codes::REVISION_NO_REPOSITORY,
            Self::NotFound { .. } => &codes::REVISION_NOT_FOUND,
            Self::Unreadable { .. } => &codes::REVISION_UNREADABLE,
        }
    }

    fn recovery(&self) -> Recovery {
        Recovery::Hint(
            match self {
                Self::NoRepository { .. } => "run starmap diff in the app's checkout",
                Self::NotFound { .. } => "run `git log` and pick a revision",
                Self::Unreadable { .. } => {
                    "check that git is installed and can read the repository"
                }
            }
            .to_owned(),
        )
    }

    fn fields(&self) -> Vec<Field<'_>> {
        match self {
            Self::NoRepository { dir } => vec![Field::new("dir", dir)],
            Self::NotFound { revision } | Self::Unreadable { revision, .. } => {
                vec![Field::new("revision", revision)]
            }
        }
    }
}

/// Reads the spec that `--spec` names and the spec that the revision holds
/// at the same path, and returns one change for each variable that was
/// added or removed and for each field that changed, in name order. A
/// revision without the file counts as an empty spec. The note names the
/// revision and its commit.
///
/// # Errors
///
/// Returns [`Failed`] when either spec cannot be read or parsed, and when
/// git cannot read the revision.
pub(crate) fn diff(args: &DiffArgs) -> Result<Batch<Change, crumbkit::Error>, Failed> {
    let path = &args.file.spec;
    let file = path.display().to_string();
    let now = spec::load(path)?;
    let revision = args.revision.as_str();
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let at = git::read_at(dir, revision, &name).map_err(|error| match error {
        GitError::NoRepository => RevisionFailure::NoRepository {
            dir: dir.display().to_string(),
        },
        GitError::NotFound => RevisionFailure::NotFound {
            revision: revision.to_owned(),
        },
        GitError::Failed(error) => RevisionFailure::Unreadable {
            revision: revision.to_owned(),
            error,
        },
    })?;
    let label = format!("{revision}:{file}");
    let short: String = at.commit.chars().take(7).collect();
    let mut note = format!("Compared {file} with {revision} ({short}).");
    let was = match at.entry {
        Entry::File(bytes) => Some(spec::parse(label, bytes)?),
        Entry::Missing => None,
        Entry::TooLarge => return Err(SpecFailure::TooLarge { file: label }.into()),
        Entry::NotFile => {
            return Err(SpecFailure::Unreadable {
                file: label,
                error: io::Error::other("the revision holds a directory at this path"),
            }
            .into());
        }
    };
    if was.is_none() {
        note.push_str(&format!(
            " {revision} has no {file}, so every variable counts as added."
        ));
    }
    // A batch answers one key with each result, so each change's key is
    // its variable's name, and a spec with no change gives a batch with no
    // key, which prints nothing on standard output.
    let changes = changes(was.as_ref(), &now);
    let keys: Vec<&str> = changes.iter().map(|change| change.name.as_str()).collect();
    let mut batch = Batch::new(&["diff"], "revision", &keys).note(note);
    batch.extend(changes.into_iter().map(Ok));
    Ok(batch)
}

/// Returns the changes from `was`, or an empty spec when it is `None`, to
/// `now`, in name order, and for a changed variable in the order of
/// [`changed_fields`].
fn changes(was: Option<&Spec>, now: &Spec) -> Vec<Change> {
    let before = |name: &Name| was.and_then(|spec| spec.variable(name));
    let names: BTreeSet<&Name> = now
        .variables()
        .map(|(name, _)| name)
        .chain(
            was.into_iter()
                .flat_map(|spec| spec.variables().map(|(name, _)| name)),
        )
        .collect();
    let mut changes = Vec::new();
    for name in names {
        let text = name.as_str().to_owned();
        match (before(name), now.variable(name)) {
            (None, Some(added)) => changes.push(Change {
                change: Kind::Added,
                name: text,
                field: None,
                was: None,
                now: Some(summary(added)),
            }),
            (Some(removed), None) => changes.push(Change {
                change: Kind::Removed,
                name: text,
                field: None,
                was: Some(summary(removed)),
                now: None,
            }),
            (Some(old), Some(new)) => {
                for (field, was, now) in changed_fields(old, new) {
                    changes.push(Change {
                        change: Kind::Changed,
                        name: text.clone(),
                        field: Some(field.to_owned()),
                        was,
                        now,
                    });
                }
            }
            (None, None) => {}
        }
    }
    changes
}

/// One field that changed: its name, its text at the revision and its
/// text in the working tree.
type FieldChange = (&'static str, Option<String>, Option<String>);

/// Returns each field that differs between `old` and `new`, in the order
/// type, unset, show, source, bind and purpose, with the text that explain
/// prints for it. The fields are compared by value and shown as text, so a
/// withheld default that changed is listed and prints as dots on both
/// sides, and diff shows no secret value. A changed source lists `source`
/// when the kind of source differs and `bind` when the bind differs.
fn changed_fields(old: &Variable, new: &Variable) -> Vec<FieldChange> {
    let mut changed = Vec::new();
    if old.form() != new.form() {
        changed.push((
            "type",
            Some(form_text(old.form())),
            Some(form_text(new.form())),
        ));
    }
    if old.unset() != new.unset() {
        changed.push(("unset", Some(unset_text(old)), Some(unset_text(new))));
    }
    if old.show() != new.show() {
        changed.push((
            "show",
            Some(old.show().to_string()),
            Some(new.show().to_string()),
        ));
    }
    if old.source() != new.source() {
        let (was, now) = (source_text(old.source()), source_text(new.source()));
        let (was_bind, now_bind) = (bind_text(old.source()), bind_text(new.source()));
        let bind_differs = was_bind != now_bind;
        if was != now || !bind_differs {
            changed.push(("source", Some(was), Some(now)));
        }
        if bind_differs {
            changed.push(("bind", was_bind, now_bind));
        }
    }
    if old.purpose() != new.purpose() {
        changed.push((
            "purpose",
            Some(old.purpose().to_owned()),
            Some(new.purpose().to_owned()),
        ));
    }
    changed
}

/// Returns the type, show and source of a variable that was added or
/// removed, such as `port, shown, setting`.
fn summary(variable: &Variable) -> String {
    format!(
        "{}, {}, {}",
        form_word(variable.form()),
        variable.show(),
        source_word(variable.source())
    )
}

/// Returns the word of a form's type, such as `integer`, without its range
/// or its choices.
fn form_word(form: &Form) -> &'static str {
    match form {
        Form::Path { .. } => "path",
        Form::Port => "port",
        Form::Url => "url",
        Form::Boolean => "boolean",
        Form::Integer(_) => "integer",
        Form::Enumeration(_) => "enumeration",
        Form::String => "string",
        // A form that a later release of starmap adds.
        _ => "another type",
    }
}

/// Returns the word of a source, such as `secret`, without its bind.
fn source_word(source: &Source) -> &'static str {
    match source {
        Source::Setting { .. } => "setting",
        Source::Secret { .. } => "secret",
        Source::Generated { .. } => "generated",
        Source::Inherited => "inherited",
        Source::Provided(_) => "provided",
        // A source that a later release of starmap adds.
        _ => "another source",
    }
}
