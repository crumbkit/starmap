#![doc = include_str!("../README.md")]

mod crumbkit_variables;
mod explained;

use std::ffi::OsString;

use crumbkit::help::{Effects, Example, Leaf};
use crumbkit::{Code, Document, Failure, Listing, Run};
use starmap::{Name, Spec, SpecError};

pub use crate::explained::{Cell, Explained, Listed, State, row};

// The text of each part of a row, which `starmap explain`, `starmap diff` and
// `starmap check` share with this crate. They are not part of the API that a
// tool uses.
#[doc(hidden)]
pub use crate::explained::{
    Values, bind_text, cache_text, empty_text, kind_text, source_text, unset_text, value_text,
};

/// The failure code of a tool whose embedded declaration does not parse.
/// The derive of `starmap::Env` refuses such a declaration when it builds the
/// tool, so only a hand-written call of [`environment`] reaches it.
pub const SPEC_INVALID: Code<'static> = Code::new("crumbkit", "environment", "spec_invalid");

/// The document that `self environment list` prints: one row for each
/// variable that the tool declares, and one for each variable that crumbkit
/// reads in every tool. Each row has the columns of `starmap explain`, a
/// top-level `state` and the `owner` of a variable that crumbkit reads.
pub const SELF_ENVIRONMENT_LIST: Document<Listing<Listed>> = Document::new(
    "crumbkit",
    "self-environment-list",
    1,
    "The environment variables that the tool declares and the library reads, each with its rules and the value it has in this process.",
)
.built_in();

/// The embedded declaration of a tool does not parse.
#[derive(Debug, Failure)]
#[failure(
    code = SPEC_INVALID,
    hint = "run `starmap check` on the starmap.toml that the tool embeds",
    field(error = error.to_string()),
)]
pub struct EnvironmentFailure {
    /// Why the declaration does not parse.
    error: SpecError,
}

impl std::fmt::Display for EnvironmentFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "the tool's embedded declaration does not parse: {}",
            self.error
        )
    }
}

impl std::error::Error for EnvironmentFailure {}

/// The examples of `self environment list`.
const LIST_EXAMPLES: &[Example] = &[
    Example::new(&[], "List the variables the tool declares and their values"),
    Example::new(
        &["--json"],
        "Print each variable as JSON, where a withheld value has no text",
    ),
];

/// Returns the leaf of `self environment list` at `path`, such as
/// `&["self", "environment", "list"]`, for the tool's help record.
#[must_use]
pub const fn list_leaf(path: &'static [&'static str]) -> Leaf {
    Leaf::new(path)
        .examples(LIST_EXAMPLES)
        .effects(Effects::read_only())
        .document(SELF_ENVIRONMENT_LIST)
}

/// Returns a row for each variable that `spec`, the tool's embedded
/// declaration, holds, and for each variable that crumbkit reads in every
/// tool, with the value that `vars` gives it, for the tool to pass to
/// [`finish`](crumbkit::finish) with [`SELF_ENVIRONMENT_LIST`]. A tool adds
/// an `environment` resource with a `list` verb that takes no arguments to its
/// `self` group, and its handler passes the result to `finish`:
///
/// ```ignore
/// SelfCommand::Environment { verb: EnvironmentVerb::List } => {
///     let result = crumbkit_starmap::environment(&mut run, <Env as starmap::Env>::SPEC, &vars);
///     crumbkit::finish(run, crumbkit_starmap::SELF_ENVIRONMENT_LIST, result)
/// }
/// ```
///
/// `spec` is `starmap::Env::SPEC` of the struct that the tool derives. `vars`
/// is the tool's own process environment, which `main` reads once. Nothing is
/// resolved: a bind is named and never read, a value of a withheld variable
/// has no text, and a variable that is unset is `unset` or `default`.
///
/// # Errors
///
/// Returns [`EnvironmentFailure`] when `spec` does not parse.
pub fn environment(
    run: &mut Run,
    spec: &str,
    vars: &[(OsString, OsString)],
) -> Result<Listing<Listed>, EnvironmentFailure> {
    let tool = run.command().get_name().to_owned();
    listing(&tool, spec, vars)
}

/// Returns the listing of `tool`'s variables in `spec` and crumbkit's, with
/// the values in `vars`.
fn listing(
    tool: &str,
    spec: &str,
    vars: &[(OsString, OsString)],
) -> Result<Listing<Listed>, EnvironmentFailure> {
    let failed = |error| EnvironmentFailure { error };
    let spec = Spec::parse(spec).map_err(failed)?;
    let crumbkit = crumbkit_variables::spec(tool).map_err(failed)?;
    let own = rows(&spec, vars);
    // A variable that the tool declares itself keeps the tool's declaration.
    let declared: Vec<String> = own.iter().map(|row| row.name().to_owned()).collect();
    let crumbkits = rows(&crumbkit, vars)
        .into_iter()
        .filter(|row| !declared.iter().any(|name| name == row.name()))
        .map(|row| row.listed(Some("crumbkit")));
    let rows: Vec<Listed> = own
        .into_iter()
        .map(|row| row.listed(None))
        .chain(crumbkits)
        .collect();
    Ok(Listing::from_all(rows, None)
        .described("variables")
        .note("Values come from this process's environment, and nothing is resolved. Each variable owned by crumbkit is read by every crumbkit tool."))
}

/// Returns the rows of every variable of `spec`, in its order.
fn rows(spec: &Spec, vars: &[(OsString, OsString)]) -> Vec<Explained> {
    let bindings = starmap::capture(spec, vars.to_vec());
    let names: Vec<Name> = spec.variables().map(|(name, _)| name.clone()).collect();
    names
        .into_iter()
        .filter_map(|name| row(spec, &bindings, name))
        .collect()
}

#[cfg(test)]
mod tests;
