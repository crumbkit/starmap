//! `starmap check`, which checks that a spec follows every rule of the file
//! and declares every variable that src reads, and names the mode that this
//! shell's values decide.

use std::ffi::OsString;
use std::fmt;
use std::path::Path;

use crumbkit::{Document, Failure, Prose};
use schemars::JsonSchema;
use serde::Serialize;
use starmap::{Mode, Name};

use crate::Failed;
use crate::cli::CheckArgs;
use crate::codes::VARIABLE_UNDECLARED;
use crate::mode;
use crate::scan;
use crate::spec;

/// The document that `starmap check` prints.
pub(crate) const CHECK: Document<Prose<Checked, Undeclared>> = Document::new(
    "starmap",
    "check",
    1,
    "A spec that follows every rule of the file: its path, digest, number of variables, the reads in src and the mode.",
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
    /// How many of the declared variables src reads.
    read: u64,
    /// The variables that src reads and the spec does not declare, each of which is also a failure.
    undeclared: Vec<String>,
    /// The src directory that check scanned, beside the spec.
    #[serde(skip)]
    src: String,
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

/// Prints one sentence, such as `starmap.toml declares 9 variables, and src
/// reads all 9.`
impl fmt::Display for Checked {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (declared, read) = (self.variables, self.read);
        let noun = if declared == 1 {
            "variable"
        } else {
            "variables"
        };
        let reads = match read {
            1 if declared == 1 => "it".to_owned(),
            0 => "none of them".to_owned(),
            _ if read == declared => format!("all {read}"),
            _ => format!("{read} of them"),
        };
        write!(
            formatter,
            "{} declares {declared} {noun}, and {} reads {reads}.",
            self.spec, self.src
        )
    }
}

/// A variable that src reads and the spec does not declare.
#[derive(Debug, Failure)]
#[failure(
    code = VARIABLE_UNDECLARED,
    hint = "declare {name} in {file}, or stop reading it",
    field(name = name.as_str()),
    field(at = at.as_str()),
)]
pub(crate) struct Undeclared {
    /// The variable.
    name: Name,
    /// The first place in src that reads it, such as `src/theme.rs:14`.
    at: String,
    /// The spec file, as the caller gave it.
    file: String,
}

impl fmt::Display for Undeclared {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} reads {}, which {} does not declare",
            self.at, self.name, self.file
        )
    }
}

impl std::error::Error for Undeclared {}

/// Reads and parses the spec that `--spec` names, scans the src directory
/// beside it, and decides the mode from `vars`, the process environment.
/// The note names the reason for the mode, and each variable that src
/// reads and the spec does not declare is a failure.
///
/// # Errors
///
/// Returns [`Failed`] when the spec cannot be read or parsed, or src cannot
/// be scanned.
pub(crate) fn check(
    args: &CheckArgs,
    vars: Vec<(OsString, OsString)>,
) -> Result<Prose<Checked, Undeclared>, Failed> {
    let path = &args.file.spec;
    let file = path.display().to_string();
    let spec = spec::load(path)?;
    let src = path.parent().unwrap_or(Path::new("")).join("src");
    let reads = scan::scan(&src)?;
    let bindings = starmap::capture(&spec, vars);
    let (mode, note) = mode::decide(&spec, &bindings);
    let mut read = 0;
    let mut undeclared = Vec::new();
    for (name, places) in reads {
        if spec.variable(&name).is_some() {
            read += 1;
        } else {
            let at = places.into_iter().next().unwrap_or_default();
            undeclared.push(Undeclared {
                name,
                at,
                file: file.clone(),
            });
        }
    }
    let checked = Checked {
        digest: spec.digest().to_string(),
        variables: u64::try_from(spec.variables().count()).unwrap_or(u64::MAX),
        read,
        undeclared: undeclared.iter().map(|u| u.name.to_string()).collect(),
        src: src.display().to_string(),
        spec: file,
        mode: mode.into(),
    };
    Ok(Prose::with_failures(checked, undeclared).note(note))
}
