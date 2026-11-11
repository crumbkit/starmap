//! `starmap check`, which checks that a spec follows every rule of the file
//! and declares every variable that src reads, and names the mode that this
//! shell's values decide.

use std::ffi::OsString;
use std::fmt;

use crumbkit::{Document, Failure, FieldValue, Prose, Recovery};
use schemars::JsonSchema;
use serde::Serialize;
use starmap::{Declaration, Mode, Name};

use crate::cli::{CheckArgs, SpecFile};
use crate::clippy;
use crate::codes::{CLIPPY_UNBANNED, FAMILY_UNDECLARED, VARIABLE_UNDECLARED};
use crate::mode;
use crate::scan::{self, Dirs};
use crate::spec;

/// The document that `starmap check` prints.
pub(crate) const CHECK: Document<Prose<Checked, Finding>> = Document::new(
    "starmap",
    "check",
    1,
    "A spec that follows every rule of the file: its path, digest, number of variables, the reads in src and the mode.",
);

/// A spec that follows every rule of the file.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Checked {
    /// The path of the file that declares the variables: the file the caller gave, or the file its pointer names.
    spec: String,
    /// The SHA-256 digest of the spec's canonical form, as `sha256:<hex>`.
    digest: String,
    /// How many variables the spec declares.
    variables: u64,
    /// How many of the declared variables src reads.
    read: u64,
    /// The variables that src reads and the spec does not declare, each of which is also a failure.
    undeclared: Vec<String>,
    /// The declared variables whose field in src names a check and that have no check sentence, each of which is also a note.
    checks_without_sentence: Vec<String>,
    /// The declared variables that have a check sentence and whose field in src names no check, each of which is also a note; empty when no source directory is scanned.
    sentences_without_check: Vec<String>,
    /// The place of each read in src whose name is built with `format!` in a shape that check cannot resolve, such as `src/tenants.rs:30`.
    unresolved: Vec<String>,
    /// The source directories that check scanned, as a sentence names
    /// them.
    #[serde(skip)]
    src: String,
    /// The verb that agrees with `src`: `reads` or `read`.
    #[serde(skip)]
    src_verb: &'static str,
    /// Whether the pointer lists no source directory, so check scanned
    /// nothing.
    #[serde(skip)]
    scanned_nothing: bool,
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
/// reads all 9.`, and a second that names each unresolved read when there
/// is one.
impl fmt::Display for Checked {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (declared, read) = (self.variables, self.read);
        let noun = if declared == 1 {
            "variable"
        } else {
            "variables"
        };
        if self.scanned_nothing {
            return write!(
                formatter,
                "{} declares {declared} {noun}, and no source directory is scanned.",
                self.spec
            );
        }
        let reads = match read {
            1 if declared == 1 => "it".to_owned(),
            0 => "none of them".to_owned(),
            _ if read == declared => format!("all {read}"),
            _ => format!("{read} of them"),
        };
        write!(
            formatter,
            "{} declares {declared} {noun}, and {} {} {reads}.",
            self.spec, self.src, self.src_verb
        )?;
        match self.unresolved.split_last() {
            None => Ok(()),
            Some((last, [])) => write!(formatter, " check cannot resolve the name read at {last}."),
            Some((last, rest)) => write!(
                formatter,
                " check cannot resolve the names read at {} and {last}.",
                rest.join(", ")
            ),
        }
    }
}

/// A finding that fails check: a read in src that the spec does not
/// declare, of a variable or of a member of a family by a name that
/// `format!` builds from a prefix, a clippy.toml that does not disallow
/// every method that reads the environment, or a clippy.toml that check
/// cannot use.
#[derive(Debug, Failure)]
pub(crate) enum Finding {
    /// A variable that src reads and the spec does not declare.
    #[failure(
        code = VARIABLE_UNDECLARED,
        hint = "declare {name} in {file}",
        alternative = Recovery::Hint(format!("stop reading {name}")),
        field(name = name.as_str()),
        field(at = at.as_str()),
    )]
    Variable {
        /// The variable.
        name: Name,
        /// The first place in src that reads it, such as `src/theme.rs:14`.
        at: String,
        /// The spec file, as the caller gave it.
        file: String,
    },
    /// A prefix that src builds names from and the spec declares no family
    /// for.
    #[failure(
        code = FAMILY_UNDECLARED,
        hint = "declare [families.{prefix}] in {file}",
        alternative = Recovery::Hint("read a declared name".to_owned()),
        field(prefix = prefix.as_str()),
        field(at = at.as_str()),
    )]
    Family {
        /// The prefix of the names.
        prefix: Name,
        /// The first place in src that reads such a name, such as
        /// `src/regions.rs:14`.
        at: String,
        /// The spec file, as the caller gave it.
        file: String,
    },
    /// A clippy.toml that Clippy reads for a crate of the application does
    /// not disallow every method of [`clippy::BANNED`], so Clippy passes a
    /// read of the environment outside the struct that derives
    /// `starmap::Env`.
    #[failure(
        code = CLIPPY_UNBANNED,
        recovery = Recovery::Run(SpecFile::call(&["init"], spec).arg("clippy")),
        field(file = file.as_str()),
        field(methods = FieldValue::list(methods.iter().copied())),
    )]
    Unbanned {
        /// The clippy.toml, or the path where init writes one.
        file: String,
        /// Whether the file exists.
        exists: bool,
        /// Each method that the file does not disallow.
        methods: Vec<&'static str>,
        /// The file that the caller gave as `--spec`.
        spec: String,
    },
    /// A clippy.toml that check cannot read or parse.
    #[failure(forward)]
    Clippy(clippy::ClippyFailure),
}

impl fmt::Display for Finding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Variable { name, at, file } => {
                write!(
                    formatter,
                    "{at} reads {name}, which {file} does not declare"
                )
            }
            Self::Family { prefix, at, file } => write!(
                formatter,
                "{at} reads names that start with {prefix}_, and {file} declares no family {prefix}"
            ),
            Self::Unbanned {
                file,
                exists: true,
                methods,
                ..
            } => write!(
                formatter,
                "{file} does not disallow {}",
                clippy::list(methods)
            ),
            Self::Unbanned {
                file,
                exists: false,
                methods,
                ..
            } => write!(
                formatter,
                "{file} does not exist, so Clippy does not disallow {}",
                clippy::list(methods)
            ),
            Self::Clippy(failure) => failure.fmt(formatter),
        }
    }
}

impl std::error::Error for Finding {}

/// Reads and parses the spec that `--spec` names, through its pointer when
/// it holds one, scans the source directories that go with it, and decides the mode from `vars`, the process environment.
/// The note names the reason for the mode, then each field in src that
/// names a check for a variable without a check sentence, then each check
/// sentence whose variable no field in src checks. Each variable that src
/// reads and the spec does not declare is a failure, and so is each prefix
/// that src builds a `format!` name from and the spec declares no family
/// for. A `format!` read that the scan cannot resolve passes, and the
/// result names its place. Each clippy.toml that Clippy reads for a
/// source directory in which the scan read a `.rs` file is a failure too
/// when it lacks an entry of [`clippy::BANNED`] or cannot be read or
/// parsed.
///
/// # Errors
///
/// Returns a [`crumbkit::Error`] when the spec cannot be read or parsed, or src cannot
/// be scanned.
pub(crate) fn check(
    args: &CheckArgs,
    vars: Vec<(OsString, OsString)>,
) -> Result<Prose<Checked, Finding>, crumbkit::Error> {
    let spec::Opened { spec, located } = spec::open(&args.file.spec)?;
    let file = located.file.display().to_string();
    let scan::Scanned {
        reads,
        checks,
        families,
        unresolved,
        rust,
    } = scan::scan_all(&located.src)?;
    let bindings = starmap::capture(&spec, vars);
    let (mode, note) = mode::decide(&spec, &bindings);
    let src = Dirs(&located.src);
    let mut notes = vec![note];
    let mut checks_without_sentence = Vec::new();
    for (name, at) in &checks {
        if spec
            .variable(name)
            .is_some_and(|variable| variable.check().is_none())
        {
            notes.push(format!(
                "{at} checks {name} in code, and {file} states no check sentence for it."
            ));
            checks_without_sentence.push(name.to_string());
        }
    }
    let mut sentences_without_check = Vec::new();
    if !src.is_empty() {
        for (name, _) in spec
            .variables()
            .filter(|(name, variable)| variable.check().is_some() && !checks.contains_key(*name))
        {
            notes.push(format!(
                "{file} states a check sentence for {name}, and no field in {src} names a check for it."
            ));
            sentences_without_check.push(name.to_string());
        }
    }
    let mut read = 0;
    let mut undeclared = Vec::new();
    let mut undeclared_names = Vec::new();
    for (name, places) in reads {
        // A read of a member's name is declared by its family, unless the
        // family lists its keys and not this one.
        let member = matches!(
            spec.declared(&name),
            Some(Declaration::Member { family, key }) if family.lists(key)
        );
        if spec.variable(&name).is_some() {
            read += 1;
        } else if !member {
            let at = places.into_iter().next().unwrap_or_default();
            undeclared_names.push(name.to_string());
            undeclared.push(Finding::Variable {
                name,
                at,
                file: file.clone(),
            });
        }
    }
    // A `format!` name with a declared prefix reads a member of that
    // family, whose key check cannot know.
    for (prefix, places) in families {
        if spec.family(&prefix).is_none() {
            let at = places.into_iter().next().unwrap_or_default();
            undeclared.push(Finding::Family {
                prefix,
                at,
                file: file.clone(),
            });
        }
    }
    for ban in clippy::open(&args.file.spec, &rust) {
        match ban {
            Ok(ban) if ban.missing.is_empty() => {}
            Ok(ban) => undeclared.push(Finding::Unbanned {
                file: ban.file.display().to_string(),
                exists: ban.exists,
                methods: ban.missing,
                spec: args.file.spec.display().to_string(),
            }),
            Err(failure) => undeclared.push(Finding::Clippy(failure)),
        }
    }
    let checked = Checked {
        digest: spec.digest().to_string(),
        variables: u64::try_from(spec.variables().count()).unwrap_or(u64::MAX),
        read,
        undeclared: undeclared_names,
        checks_without_sentence,
        sentences_without_check,
        unresolved,
        src: Dirs(&located.src).to_string(),
        src_verb: Dirs(&located.src).verb("reads", "read"),
        scanned_nothing: Dirs(&located.src).is_empty(),
        spec: file,
        mode: mode.into(),
    };
    Ok(Prose::with_failures(checked, undeclared).note(notes.join("\n")))
}
