//! `starmap check`, which checks that a spec follows every rule of the file
//! and names the mode that this shell's values decide.

use std::ffi::OsString;
use std::fmt;

use crumbkit::{Document, Prose};
use schemars::JsonSchema;
use serde::Serialize;
use starmap::Mode;

use crate::cli::CheckArgs;
use crate::mode;
use crate::spec::{self, SpecFailure};

/// The document that `starmap check` prints.
pub(crate) const CHECK: Document<Prose<Checked>> = Document::new(
    "starmap",
    "check",
    1,
    "A spec that follows every rule of the file: its path, digest, number of variables and mode.",
);

/// A spec that follows every rule of the file.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Checked {
    /// The path of the spec, as the caller gave it.
    spec: String,
    /// The SHA-256 digest of the spec's canonical form, as `sha256:<hex>`.
    digest: String,
    /// How many variables the spec declares.
    variables: u64,
    /// The mode that this shell's values decide.
    mode: ModeWord,
}

/// The mode an application runs in, as the JSON document names it.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ModeWord {
    /// The mode of a developer's machine.
    Local,
    /// The mode that the spec's production values select.
    Production,
}

impl From<Mode> for ModeWord {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Local => Self::Local,
            Mode::Production => Self::Production,
        }
    }
}

/// Prints one sentence, such as `starmap.toml declares 9 variables.`
impl fmt::Display for Checked {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let noun = if self.variables == 1 {
            "variable"
        } else {
            "variables"
        };
        write!(
            formatter,
            "{} declares {} {noun}.",
            self.spec, self.variables
        )
    }
}

/// Reads and parses the spec that `--spec` names, and decides the mode from
/// `vars`, the process environment. The note names the reason for the
/// mode.
///
/// # Errors
///
/// Returns [`SpecFailure`] when the spec cannot be read or parsed.
pub(crate) fn check(
    args: &CheckArgs,
    vars: Vec<(OsString, OsString)>,
) -> Result<Prose<Checked>, SpecFailure> {
    let spec = spec::load(&args.file.spec)?;
    let bindings = starmap::capture(&spec, vars);
    let (mode, note) = mode::decide(&spec, &bindings);
    let checked = Checked {
        spec: args.file.spec.display().to_string(),
        digest: spec.digest().to_string(),
        variables: u64::try_from(spec.variables().count()).unwrap_or(u64::MAX),
        mode: mode.into(),
    };
    Ok(Prose::new(checked).note(note))
}
