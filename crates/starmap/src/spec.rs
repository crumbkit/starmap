//! The parsed `starmap.toml`: its variables, its mode rule, its canonical
//! form and digest, and the problems that stop a file from parsing.

mod canonical;
mod parse;

use std::error::Error;
use std::fmt;
use std::ops::Range;

use crate::{Form, Mode, ModeRule, Name, PathTemplate, ProvidedRef, SecretBytes, SecretRef, Value};

#[cfg(test)]
mod tests;

/// A parsed `starmap.toml`: each variable an application reads, in the
/// order the file lists them, and the rule that decides the mode.
///
/// [`Spec::parse`] is the only way to build one, and it checks every rule
/// of the file, so a `Spec` holds no combination the rules forbid.
///
/// ```
/// let spec = starmap::Spec::parse(
///     r#"
/// [variables.SHELF_PORT]
/// purpose = "The port the api process listens on."
/// type = "port"
/// unset = { default = "8080" }
/// show = "shown"
/// source = "setting"
/// "#,
/// )?;
/// let name: starmap::Name = "SHELF_PORT".parse()?;
/// assert_eq!(spec.variable(&name).map(|variable| variable.form()), Some(&starmap::Form::Port));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    mode: Option<ModeRule>,
    variables: Vec<(Name, Variable)>,
}

impl Spec {
    /// The most bytes a `starmap.toml` may have, which is 256 KiB. A larger
    /// file is refused before it is parsed.
    pub const MAX_BYTES: usize = 256 * 1024;

    /// Parses the text of a `starmap.toml`.
    ///
    /// # Errors
    ///
    /// Returns [`SpecError`] with every problem the parser finds when the
    /// text is over [`Spec::MAX_BYTES`], is not TOML, or breaks a rule of
    /// the file.
    pub fn parse(text: &str) -> Result<Self, SpecError> {
        parse::parse(text)
    }

    /// Returns the rule that decides the mode, if the spec has a `[mode]`
    /// table. Without one, an application always runs in local mode.
    #[must_use]
    pub fn mode(&self) -> Option<&ModeRule> {
        self.mode.as_ref()
    }

    /// Returns the variable called `name`, if the spec declares it.
    #[must_use]
    pub fn variable(&self, name: &Name) -> Option<&Variable> {
        self.variables
            .iter()
            .find(|(declared, _)| declared == name)
            .map(|(_, variable)| variable)
    }

    /// Returns each declared variable with its name, in the order the file
    /// lists them.
    pub fn variables(&self) -> impl Iterator<Item = (&Name, &Variable)> {
        self.variables
            .iter()
            .map(|(name, variable)| (name, variable))
    }

    /// Returns the SHA-256 digest of the spec's canonical form, which
    /// [`Spec::to_toml`] returns. Two files that differ only in layout,
    /// comments or the spelling of equal values have the same digest. The
    /// canonical form keeps the file's order of variables, so reordering
    /// them changes the digest.
    #[must_use]
    pub fn digest(&self) -> Digest {
        Digest::of(self.to_toml().as_bytes())
    }

    /// Returns the spec's canonical form: TOML that lists the mode table and
    /// then each variable in order, with each field in a fixed order, each
    /// value in one spelling, and no comments. Parsing it returns an equal
    /// spec.
    #[must_use]
    pub fn to_toml(&self) -> String {
        canonical::to_toml(self)
    }
}

/// One variable that a `starmap.toml` declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    purpose: String,
    form: Form,
    unset: UnsetRule,
    show: Show,
    source: Source,
}

impl Variable {
    /// Returns the sentence that says what the variable is for.
    #[must_use]
    pub fn purpose(&self) -> &str {
        &self.purpose
    }

    /// Returns the form the variable's value takes, which the file's `type`
    /// field states.
    #[must_use]
    pub fn form(&self) -> &Form {
        &self.form
    }

    /// Returns what happens when the variable has no value, in each mode.
    #[must_use]
    pub fn unset(&self) -> &UnsetRule {
        &self.unset
    }

    /// Returns whether the variable's value may be shown.
    #[must_use]
    pub fn show(&self) -> Show {
        self.show
    }

    /// Returns where the variable's value comes from, with its bind.
    #[must_use]
    pub fn source(&self) -> &Source {
        &self.source
    }
}

/// Where a variable's value comes from, merged with the bind that the
/// source allows, so that a combination the file's rules forbid cannot be
/// built (BASE-45).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Source {
    /// A value the person or the host sets, with an optional path bind that
    /// supplies it from a template.
    Setting {
        /// The template a host expands into the value.
        bind: Option<PathTemplate>,
    },
    /// A secret value, with an optional reference that a resolver reads it
    /// from.
    Secret {
        /// The reference a resolver reads the value from.
        bind: Option<SecretRef>,
    },
    /// A random value that the host creates on first use and keeps.
    Generated {
        /// How many random bytes the value has.
        bytes: SecretBytes,
    },
    /// A value the application inherits from the environment it starts in,
    /// such as `HOME`.
    Inherited,
    /// A value a provider of the bind's kind supplies.
    Provided(ProvidedRef),
}

/// Whether a variable's value may be shown, such as in `starmap explain`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Show {
    /// The value may be shown.
    Shown,
    /// The value is never shown, and it is read as a secret.
    Withheld,
}

impl fmt::Display for Show {
    /// Writes the word a `starmap.toml` uses: `shown` or `withheld`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Shown => "shown",
            Self::Withheld => "withheld",
        })
    }
}

/// What happens when a variable has no value, in each mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsetRule {
    local: Unset,
    /// Equal to `local` when the file states no production rule, and
    /// otherwise [`Unset::Required`] or [`Unset::Optional`], because a
    /// production rule takes no default of its own.
    production: Unset,
}

impl UnsetRule {
    /// Returns the rule in `mode`.
    #[must_use]
    pub fn in_mode(&self, mode: Mode) -> &Unset {
        match mode {
            Mode::Local => &self.local,
            Mode::Production => &self.production,
        }
    }
}

/// What happens when a variable has no value in one mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unset {
    /// The variable must have a value.
    Required,
    /// The variable may stay unset.
    Optional,
    /// The variable takes this value, which fits its form.
    Default(Value),
}

/// The SHA-256 digest of a spec's canonical form. It prints as
/// `sha256:<hex>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Digest([u8; 32]);

impl Digest {
    /// Computes the digest of `bytes`.
    fn of(bytes: &[u8]) -> Self {
        use sha2::Digest as _;
        Self(sha2::Sha256::digest(bytes).into())
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sha256:")?;
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// The problems that stop a `starmap.toml` from parsing. It holds at least
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecError {
    problems: Vec<SpecProblem>,
}

impl SpecError {
    /// Returns every problem, in the order the parser found them.
    #[must_use]
    pub fn problems(&self) -> &[SpecProblem] {
        &self.problems
    }
}

impl fmt::Display for SpecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, problem) in self.problems.iter().enumerate() {
            if index > 0 {
                formatter.write_str("; ")?;
            }
            problem.fmt(formatter)?;
        }
        Ok(())
    }
}

impl Error for SpecError {}

/// One problem in a `starmap.toml`: its kind, the span of text it is
/// about, and a message that names the variable or table and the rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecProblem {
    span: Range<usize>,
    kind: SpecProblemKind,
    message: String,
}

impl SpecProblem {
    /// Returns the range of bytes in the file's text that the problem is
    /// about. It is empty when the problem has no place, as for a file
    /// that is too large.
    #[must_use]
    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }

    /// Returns the problem's kind.
    #[must_use]
    pub fn kind(&self) -> SpecProblemKind {
        self.kind
    }
}

impl fmt::Display for SpecProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// The kind of a [`SpecProblem`]. Each kind has a code, which the CLI
/// reports as `starmap::spec::<code>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SpecProblemKind {
    /// The file is over [`Spec::MAX_BYTES`].
    TooLarge,
    /// The file is not valid TOML.
    Syntax,
    /// A table holds a key that starmap does not know.
    UnknownField,
    /// A table lacks a key it needs, such as a variable without `purpose`.
    MissingField,
    /// A value has the wrong TOML type, or is not one of the values its key
    /// takes.
    BadValue,
    /// A variable name is not upper case letters, digits and underscores.
    BadName,
    /// A value still ends in the `?` that `starmap init` wrote, or a
    /// purpose is `?`.
    Guess,
    /// A default does not fit the variable's type.
    BadDefault,
    /// An integer type's minimum is above its maximum.
    BadRange,
    /// An enumeration, or the mode's production list, has no values or
    /// repeats one.
    EmptyEnumeration,
    /// A secret or generated variable is shown.
    ShownSecret,
    /// A withheld variable has a type other than string.
    SecretForm,
    /// A bind does not fit the source, or a provided source has no
    /// provided bind.
    BindMismatch,
    /// `bytes` lies outside 16 to 1024, or appears on a variable that is
    /// not generated.
    BadBytes,
    /// A path template names a placeholder other than `{workspace}`,
    /// `{checkout}` and `{home}`, or has a brace outside one.
    UnknownPlaceholder,
    /// `[mode]` names a variable the file does not declare.
    ModeUndeclared,
    /// The mode variable is not a setting or inherited variable.
    ModeSource,
}

impl SpecProblemKind {
    /// Returns the kind's code, such as `bad_default`, which the CLI
    /// reports as `starmap::spec::bad_default`.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::TooLarge => "too_large",
            Self::Syntax => "syntax",
            Self::UnknownField => "unknown_field",
            Self::MissingField => "missing_field",
            Self::BadValue => "bad_value",
            Self::BadName => "bad_name",
            Self::Guess => "guess",
            Self::BadDefault => "bad_default",
            Self::BadRange => "bad_range",
            Self::EmptyEnumeration => "empty_enumeration",
            Self::ShownSecret => "shown_secret",
            Self::SecretForm => "secret_form",
            Self::BindMismatch => "bind_mismatch",
            Self::BadBytes => "bad_bytes",
            Self::UnknownPlaceholder => "unknown_placeholder",
            Self::ModeUndeclared => "mode_undeclared",
            Self::ModeSource => "mode_source",
        }
    }
}

/// The word of a variable's `source` field, before the bind is merged in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Setting,
    Secret,
    Generated,
    Inherited,
    Provided,
}

/// The words a type takes, with the form each names. The parser reads them,
/// and the canonical form writes them.
const FORM_WORDS: &[(&str, Form)] = &[
    ("path", Form::Path { absolute: false }),
    ("port", Form::Port),
    ("url", Form::Url),
    ("boolean", Form::Boolean),
    ("string", Form::String),
];
/// The words an unset rule takes in one mode.
const UNSET_WORDS: &[(&str, Unset)] =
    &[("required", Unset::Required), ("optional", Unset::Optional)];
/// The words `show` takes.
const SHOW_WORDS: &[(&str, Show)] = &[("shown", Show::Shown), ("withheld", Show::Withheld)];
/// The words `source` takes.
const SOURCE_WORDS: &[(&str, SourceKind)] = &[
    ("setting", SourceKind::Setting),
    ("secret", SourceKind::Secret),
    ("generated", SourceKind::Generated),
    ("inherited", SourceKind::Inherited),
    ("provided", SourceKind::Provided),
];

/// Returns the word that names `value` in `words`. Each table lists every
/// value it is used with, so the empty text it returns otherwise is never
/// written.
fn word_of<T: PartialEq>(words: &[(&'static str, T)], value: &T) -> &'static str {
    words
        .iter()
        .find(|(_, listed)| listed == value)
        .map_or("", |(word, _)| word)
}
