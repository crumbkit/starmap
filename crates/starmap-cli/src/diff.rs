//! `starmap diff`, which compares the spec in the working tree with the
//! spec that a git revision holds, and lists each variable that was added,
//! changed or removed.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::path::Path;

use crumbkit::{Document, Failure, Listing, Row};
use schemars::JsonSchema;
use serde::Serialize;
use starmap::{Name, Source, Spec, Type, Variable};

use crate::cli::DiffArgs;
use crate::codes;
use crate::explain::{Values, bind_text, cache_text, source_text, type_text, unset_text};
use crate::git::{self, Entry, GitError};
use crate::spec::{self, SpecFailure};

#[cfg(test)]
mod tests;

/// The document that `starmap diff` prints.
pub(crate) const DIFF: Document<Listing<Change>> = Document::new(
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
#[derive(Debug, Failure)]
pub(crate) enum RevisionFailure {
    /// The spec's directory is not in a git repository.
    #[failure(
        code = codes::REVISION_NO_REPOSITORY,
        hint = "run starmap diff in the app's checkout",
        field(dir = dir.as_str()),
    )]
    NoRepository {
        /// The spec's directory, or an empty string for the current
        /// directory.
        dir: String,
    },
    /// The revision names no commit.
    #[failure(
        code = codes::REVISION_NOT_FOUND,
        hint = "run `git log` and pick a revision",
        field(revision = revision.as_str()),
    )]
    NotFound {
        /// The revision, as the caller gave it.
        revision: String,
    },
    /// git cannot start or cannot read the revision.
    #[failure(
        code = codes::REVISION_UNREADABLE,
        hint = "check that git is installed and can read the repository",
        field(revision = revision.as_str()),
    )]
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

/// Reads the spec that `--spec` names, through its pointer when it holds
/// one, and the spec that the revision holds at the same path, and returns one change for each variable that was
/// added or removed and for each field that changed, in name order. A
/// revision without the file counts as an empty spec. The note names the
/// revision and its commit.
///
/// # Errors
///
/// Returns a [`crumbkit::Error`] when either spec cannot be read or parsed, and when
/// git cannot read the revision.
pub(crate) fn diff(args: &DiffArgs) -> Result<Listing<Change>, crumbkit::Error> {
    let spec::Opened { spec: now, located } = spec::open(&args.file.spec)?;
    let path = &located.file;
    let file = path.display().to_string();
    let revision = args.revision.as_str();
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    let Some(name) = path.file_name().and_then(OsStr::to_str) else {
        return Err(SpecFailure::Unreadable {
            file,
            error: io::Error::other("the file name is not UTF-8, so git cannot look it up"),
        }
        .into());
    };
    let at = git::read_at(dir, revision, name).map_err(|error| match error {
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
                error: io::Error::other(
                    "the revision holds a directory, a symlink or a submodule at this path",
                ),
            }
            .into());
        }
    };
    if was.is_none() {
        note.push_str(&format!(
            " {revision} has no {file}, so every variable counts as added."
        ));
    }
    let changes = changes(was.as_ref(), &now);
    let count = changes.len();
    let noun = if count == 1 { "change" } else { "changes" };
    Ok(Listing::new(changes, count).note(note).described(noun))
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
                let group = group_change(was, now, name);
                for (field, was, now) in changed_fields(old, new).into_iter().chain(group) {
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
/// type, values, unset, show, source, bind, cache, purpose and effect, with
/// the text that explain prints for it. A changed type lists `type` when
/// its text differs, and `values` when the descriptions of a named
/// enumeration's values differ. The fields are compared by value and shown as
/// text, and a withheld variable has no default, so diff shows no secret
/// value. A changed source lists
/// `source` when the kind of source differs, `bind` when the bind's
/// reference differs, and `cache` when whether a cache may keep the value
/// differs, which reads `allowed` or `never`.
fn changed_fields(old: &Variable, new: &Variable) -> Vec<FieldChange> {
    let mut changed = Vec::new();
    if old.value_type() != new.value_type() {
        let (was, now) = (type_text(old.value_type()), type_text(new.value_type()));
        let text = |variable: &Variable| Values::of(variable.value_type()).map(|v| v.to_string());
        let (was_values, now_values) = (text(old), text(new));
        let values_differ = was_values != now_values;
        if was != now || !values_differ {
            changed.push(("type", Some(was), Some(now)));
        }
        if values_differ {
            changed.push(("values", was_values, now_values));
        }
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
        let (was_cache, now_cache) = (cache_text(old.source()), cache_text(new.source()));
        let bind_differs = was_bind != now_bind;
        let cache_differs = was_cache != now_cache;
        if was != now || !(bind_differs || cache_differs) {
            changed.push(("source", Some(was), Some(now)));
        }
        if bind_differs {
            changed.push(("bind", was_bind, now_bind));
        }
        if cache_differs {
            let word = |cache: Option<&str>| Some(cache.unwrap_or("allowed").to_owned());
            changed.push(("cache", word(was_cache), word(now_cache)));
        }
    }
    if old.purpose() != new.purpose() {
        changed.push((
            "purpose",
            Some(old.purpose().to_owned()),
            Some(new.purpose().to_owned()),
        ));
    }
    if old.effect() != new.effect() {
        changed.push((
            "effect",
            old.effect().map(str::to_owned),
            new.effect().map(str::to_owned),
        ));
    }
    changed
}

/// Returns the change of the group that lists `name`, when it differs
/// between `was` and `now`, as the field `group`, which diff lists after
/// the variable's own fields.
fn group_change(was: Option<&Spec>, now: &Spec, name: &Name) -> Option<FieldChange> {
    let group = |spec: &Spec| spec.group_of(name).map(|group| group.name().to_string());
    let (before, after) = (was.and_then(group), group(now));
    (before != after).then_some(("group", before, after))
}

/// Returns the type, show and source of a variable that was added or
/// removed, such as `port, shown, setting`.
fn summary(variable: &Variable) -> String {
    format!(
        "{}, {}, {}",
        type_word(variable.value_type()),
        variable.show(),
        source_word(variable.source())
    )
}

/// Returns the word of a type, such as `integer`, without its range or its
/// choices.
fn type_word(ty: &Type) -> &'static str {
    match ty {
        Type::Path { .. } => "path",
        Type::Port => "port",
        Type::Url { .. } => "url",
        Type::SocketAddress => "ip socket address",
        Type::Boolean => "boolean",
        Type::Integer(_) => "integer",
        Type::Enumeration(_) => "enumeration",
        Type::String { .. } => "string",
        Type::Bytes(_) => "bytes",
        // A type that a later release of starmap adds.
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
