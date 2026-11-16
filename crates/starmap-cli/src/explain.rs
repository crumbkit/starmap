//! `starmap explain`, which describes each declared variable and the value
//! it has in this shell.

use std::ffi::OsString;
use std::fmt;

use crumbkit::{Batch, Document, Failure, Recovery};
use starmap::{Bindings, BuiltIns, Name, Spec};

pub(crate) use crumbkit_starmap::{
    Explained, Values, bind_text, cache_text, empty_text, kind_text, source_text, unset_text,
    value_text,
};

use crate::cli::{ExplainArgs, SpecFile};
use crate::codes::VARIABLE_UNKNOWN;
use crate::mode;
use crate::spec::{self, SpecFailure};

#[cfg(test)]
mod tests;

/// The document that `starmap explain` prints.
pub(crate) const EXPLAIN: Document<Batch<Explained, UnknownVariable>> = Document::new(
    "starmap",
    "explain",
    1,
    "Each variable that a spec declares, with its rules and the value it has in this shell.",
);

/// A name that `starmap explain` was given and the spec does not declare.
#[derive(Debug, Failure)]
#[failure(
    code = VARIABLE_UNKNOWN,
    recovery = list_again(file),
    field(name = name.as_str()),
    field(file = file.as_str()),
)]
pub(crate) struct UnknownVariable {
    /// The name.
    name: Name,
    /// The spec file, as the caller gave it.
    file: String,
}

impl fmt::Display for UnknownVariable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} does not declare {}", self.file, self.name)
    }
}

/// Returns the recovery of an unknown name, which explains every variable
/// of the same spec file.
fn list_again(file: &str) -> Recovery {
    Recovery::Run(SpecFile::call(&["explain"], file))
}

impl std::error::Error for UnknownVariable {}

/// Reads and parses the spec that `--spec` names, and explains each
/// variable that `names` lists, or every declared variable without names,
/// with its value in `vars`, the process environment. The note names the
/// mode and says that explain resolves no bind. Without names, the note
/// also lists the built-in variables that `starmap run --only-declared`
/// passes.
///
/// # Errors
///
/// Returns [`SpecFailure`] when the spec cannot be read or parsed. A name
/// that the spec does not declare is a failure of that name alone.
pub(crate) fn explain(
    args: &ExplainArgs,
    vars: Vec<(OsString, OsString)>,
) -> Result<Batch<Explained, UnknownVariable>, SpecFailure> {
    let spec = spec::open(&args.file.spec)?.spec;
    let file = args.file.spec.display().to_string();
    let bindings = starmap::capture(&spec, vars);
    let names: Vec<Name> = if args.names.is_empty() {
        spec.variables().map(|(name, _)| name.clone()).collect()
    } else {
        args.names.clone()
    };
    let (_, note) = mode::decide(&spec, &bindings);
    let mut note = format!("{note} Values come from this shell, and explain resolves no bind.");
    if args.names.is_empty() {
        note.push(' ');
        note.push_str(&built_ins_sentence(BuiltIns::current()));
    }
    let mut batch = Batch::new(&["explain"], "names", &names).note(note);
    for name in names {
        batch.push(row(&spec, &file, &bindings, name));
    }
    Ok(batch)
}

/// Returns the sentence that lists `built_ins`, the variables that
/// `starmap run --only-declared` passes to the program without a
/// declaration.
fn built_ins_sentence(built_ins: &BuiltIns) -> String {
    let names: Vec<&str> = built_ins.names().collect();
    format!(
        "starmap run --only-declared also passes these built-in variables from the shell: {}.",
        names.join(", ")
    )
}

/// Returns the row of the variable `name`, which crumbkit-starmap builds
/// for `self environment list` too, or the failure of a name that `spec`,
/// read from `file`, does not declare.
fn row(
    spec: &Spec,
    file: &str,
    bindings: &Bindings,
    name: Name,
) -> Result<Explained, UnknownVariable> {
    crumbkit_starmap::row(spec, bindings, name.clone()).ok_or_else(|| UnknownVariable {
        name,
        file: file.to_owned(),
    })
}
