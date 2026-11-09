//! The parsed `starmap.toml`: its variables, its mode rule, its canonical
//! form and digest, and the problems that stop a file from parsing.

mod canonical;
mod parse;

use std::error::Error;
use std::fmt;
use std::ops::Range;

use crate::{
    Mode, ModeRule, Name, PathTemplate, ProvidedRef, SecretBind, SecretBytes, Type, Value,
};

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
/// assert_eq!(spec.variable(&name).map(|variable| variable.value_type()), Some(&starmap::Type::Port));
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
    value_type: Type,
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

    /// Returns the type of the variable's value, which the file's `type`
    /// field states.
    #[must_use]
    pub fn value_type(&self) -> &Type {
        &self.value_type
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
    /// A secret value, with an optional bind that names the reference a
    /// resolver reads it from.
    Secret {
        /// The reference a resolver reads the value from, and whether a
        /// cache may keep the value.
        bind: Option<SecretBind>,
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
    /// The variable takes this value, which fits its type.
    Default(Value),
}

impl fmt::Display for Unset {
    /// Writes `required`, `optional` or `default`, as a program's `--help`
    /// describes a variable. It never writes the default's value, so a
    /// caller that describes a variable writes the [`Value`] after it.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Required => "required",
            Self::Optional => "optional",
            Self::Default(_) => "default",
        })
    }
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

/// The kind of a [`SpecProblem`]. Each kind has a code of the form
/// `starmap::spec::<name>`, which [`SpecProblemKind::code`] returns.
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
    /// A withheld variable has a default, which `starmap.toml` would hold in
    /// plain text.
    WithheldDefault,
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
    /// Returns the kind's code, such as `starmap::spec::bad_default`.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::TooLarge => "starmap::spec::too_large",
            Self::Syntax => "starmap::spec::syntax",
            Self::UnknownField => "starmap::spec::unknown_field",
            Self::MissingField => "starmap::spec::missing_field",
            Self::BadValue => "starmap::spec::bad_value",
            Self::BadName => "starmap::spec::bad_name",
            Self::Guess => "starmap::spec::guess",
            Self::BadDefault => "starmap::spec::bad_default",
            Self::BadRange => "starmap::spec::bad_range",
            Self::EmptyEnumeration => "starmap::spec::empty_enumeration",
            Self::ShownSecret => "starmap::spec::shown_secret",
            Self::SecretForm => "starmap::spec::secret_form",
            Self::WithheldDefault => "starmap::spec::withheld_default",
            Self::BindMismatch => "starmap::spec::bind_mismatch",
            Self::BadBytes => "starmap::spec::bad_bytes",
            Self::UnknownPlaceholder => "starmap::spec::unknown_placeholder",
            Self::ModeUndeclared => "starmap::spec::mode_undeclared",
            Self::ModeSource => "starmap::spec::mode_source",
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

/// The words a type takes, with the type each names. The parser reads them,
/// and the canonical form writes them.
const TYPE_WORDS: &[(&str, Type)] = &[
    ("path", Type::Path { absolute: false }),
    ("port", Type::Port),
    ("url", Type::Url),
    ("socket-address", Type::SocketAddress),
    ("boolean", Type::Boolean),
    ("string", Type::String),
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
