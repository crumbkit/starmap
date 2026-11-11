//! The parsed `starmap.toml`: its variables, its mode rule, its canonical
//! form and digest, and the problems that stop a file from parsing.

mod canonical;
mod group;
mod parse;

use std::error::Error;
use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::{
    ByteCount, Choices, Encoding, EnumerationName, Mode, ModeRule, Name, PathTemplate, ProvidedRef,
    SecretBind, Type, Value,
};

pub use group::{Group, GroupName, GroupNameError};

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
    /// The file's named enumerations, in name order. Each one has a name.
    enumerations: Vec<Choices>,
    /// The file's groups of variables, in name order.
    groups: Vec<Group>,
    variables: Vec<(Name, Variable)>,
    /// The file's top-level tool tables, in name order.
    tools: Vec<ToolTable>,
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
    /// the file. A [`Pointer`] fails as
    /// [`SpecProblemKind::PointerChain`], or as
    /// [`SpecProblemKind::PointerDeclares`] when it declares variables too.
    pub fn parse(text: &str) -> Result<Self, SpecError> {
        parse::parse(text)
    }

    /// Returns the rule that decides the mode, if the spec has a `[mode]`
    /// table. Without one, an application always runs in local mode.
    #[must_use]
    pub fn mode(&self) -> Option<&ModeRule> {
        self.mode.as_ref()
    }

    /// Returns the named enumeration called `name`, if the spec declares
    /// it in an `[enumerations.<name>]` table.
    #[must_use]
    pub fn enumeration(&self, name: &EnumerationName) -> Option<&Choices> {
        self.enumerations
            .iter()
            .find(|choices| choices.name() == Some(name))
    }

    /// Returns each named enumeration the spec declares, in name order,
    /// including those that no variable uses.
    pub fn enumerations(&self) -> impl Iterator<Item = &Choices> {
        self.enumerations.iter()
    }

    /// Returns each group of variables the spec declares, in name order.
    pub fn groups(&self) -> impl Iterator<Item = &Group> {
        self.groups.iter()
    }

    /// Returns the group called `name`, if the spec declares it in a
    /// `[groups.<name>]` table.
    #[must_use]
    pub fn group(&self, name: &GroupName) -> Option<&Group> {
        self.groups.iter().find(|group| group.name() == name)
    }

    /// Returns the group that lists the variable `variable`, if one does. A
    /// variable belongs to at most one group.
    #[must_use]
    pub fn group_of(&self, variable: &Name) -> Option<&Group> {
        self.groups
            .iter()
            .find(|group| group.variables().contains(variable))
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

    /// Returns the file's top-level `[tool.<name>]` table called `name`, if
    /// it has one.
    #[must_use]
    pub fn tool(&self, name: &str) -> Option<&ToolTable> {
        find_tool(&self.tools, name)
    }

    /// Returns each top-level `[tool.<name>]` table, in name order.
    pub fn tools(&self) -> impl Iterator<Item = &ToolTable> {
        self.tools.iter()
    }

    /// Returns the SHA-256 digest of the spec's canonical form, which
    /// [`Spec::to_toml`] returns. Two files that differ only in layout,
    /// comments, the spelling of equal values or their tool tables have
    /// the same digest. The canonical form keeps the file's order of
    /// variables, so reordering them changes the digest.
    #[must_use]
    pub fn digest(&self) -> Digest {
        Digest::of(self.to_toml().as_bytes())
    }

    /// Returns the spec's canonical form: TOML that lists the mode table,
    /// then each named enumeration in name order with its values in the
    /// file's order, then each group in name order with its variables in
    /// the file's order, and then each variable in order, with each field in a fixed order, each
    /// value in one spelling, and no comments. It leaves out every tool
    /// table, so parsing it returns a spec equal to this one without its
    /// tool tables.
    #[must_use]
    pub fn to_toml(&self) -> String {
        canonical::to_toml(self)
    }
}

/// A `starmap.toml` that points to the file that declares the variables,
/// in place of declaring them itself. A file is a pointer when its top
/// level has the key `spec`:
///
/// ```toml
/// spec = "config/environment.toml"
/// src = ["src"]
/// dotenv = ".env"
/// ```
///
/// Each path is as the file writes it, relative to the directory of the
/// pointer file, and the host joins it to that directory. `src` lists the
/// directories whose source `starmap check` and `starmap init` scan, where
/// an empty list scans nothing, and
/// `dotenv` names the file that `starmap run` reads values from.
///
/// ```
/// let pointer = starmap::Pointer::parse("spec = \"config/environment.toml\"\n")?
///     .ok_or("the file declares variables")?;
/// assert_eq!(pointer.spec(), std::path::Path::new("config/environment.toml"));
/// assert_eq!(pointer.src(), [std::path::PathBuf::from("src")]);
/// assert_eq!(pointer.dotenv(), std::path::Path::new(".env"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pointer {
    spec: PathBuf,
    src: Vec<PathBuf>,
    dotenv: PathBuf,
    /// The pointer's top-level tool tables, in name order.
    tools: Vec<ToolTable>,
}

impl Pointer {
    /// The source directory when the pointer lists none.
    pub const SRC: &'static str = "src";
    /// The `.env` file when the pointer names none.
    pub const DOTENV: &'static str = ".env";

    /// Parses the text of a `starmap.toml` as a pointer. It returns `None`
    /// when the file has no top-level `spec` key, because such a file
    /// declares variables, and [`Spec::parse`] reads it.
    ///
    /// # Errors
    ///
    /// Returns [`SpecError`] when the text is over [`Spec::MAX_BYTES`] or
    /// is not TOML, and for a pointer that also declares variables
    /// ([`SpecProblemKind::PointerDeclares`]), holds a key that a pointer
    /// does not take, or gives a path that is not text or is empty.
    pub fn parse(text: &str) -> Result<Option<Self>, SpecError> {
        parse::parse_pointer(text)
    }

    /// Returns the path of the file that declares the variables.
    #[must_use]
    pub fn spec(&self) -> &Path {
        &self.spec
    }

    /// Returns the source directories that a scan reads, which are `src`
    /// unless the pointer lists others.
    #[must_use]
    pub fn src(&self) -> &[PathBuf] {
        &self.src
    }

    /// Returns the `.env` file, which is `.env` unless the pointer names
    /// another.
    #[must_use]
    pub fn dotenv(&self) -> &Path {
        &self.dotenv
    }

    /// Returns the pointer's `[tool.<name>]` table called `name`, if it has
    /// one. starmap never merges it into the tool tables of the file the
    /// pointer names.
    #[must_use]
    pub fn tool(&self, name: &str) -> Option<&ToolTable> {
        find_tool(&self.tools, name)
    }

    /// Returns each of the pointer's `[tool.<name>]` tables, in name order.
    pub fn tools(&self) -> impl Iterator<Item = &ToolTable> {
        self.tools.iter()
    }
}

/// One `[tool.<name>]` table of a `starmap.toml`, where a tool such as a
/// generator or a linter keeps its own settings. starmap checks only that
/// the table is a table, and never reads its keys. The table's text is a
/// TOML document that the tool parses with its own TOML crate.
///
/// ```
/// let spec = starmap::Spec::parse(
///     r#"
/// [tool.codegen]
/// module = "settings"
///
/// [variables.SHELF_PORT]
/// purpose = "The port the api process listens on."
/// type = "port"
/// unset = "required"
/// show = "shown"
/// source = "setting"
/// tool.codegen = { field = "port" }
/// "#,
/// )?;
/// assert_eq!(spec.tool("codegen").map(starmap::ToolTable::as_toml), Some("module = \"settings\"\n"));
/// let name: starmap::Name = "SHELF_PORT".parse()?;
/// let variable = spec.variable(&name).ok_or("SHELF_PORT is declared")?;
/// assert_eq!(variable.tool("codegen").map(starmap::ToolTable::as_toml), Some("field = \"port\"\n"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolTable {
    name: String,
    toml: String,
}

impl ToolTable {
    /// Returns the tool's name, which is the key after `tool.`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the table's keys and values as the text of a TOML document,
    /// in the file's order, without its comments or layout.
    #[must_use]
    pub fn as_toml(&self) -> &str {
        &self.toml
    }
}

/// Returns the tool table called `name` among `tools`.
fn find_tool<'a>(tools: &'a [ToolTable], name: &str) -> Option<&'a ToolTable> {
    tools.iter().find(|tool| tool.name == name)
}

/// One variable that a `starmap.toml` declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    purpose: String,
    /// The sentence that says what the application does when the variable
    /// is unset, which the file's optional `effect` field states.
    effect: Option<String>,
    /// The sentence that states the application's own check of the value,
    /// which the file's optional `check` field states.
    check: Option<String>,
    value_type: Type,
    unset: UnsetRule,
    show: Show,
    source: Source,
    /// The variable's tool tables, in name order.
    tools: Vec<ToolTable>,
}

impl Variable {
    /// Returns the sentence that says what the variable is for.
    #[must_use]
    pub fn purpose(&self) -> &str {
        &self.purpose
    }

    /// Returns the sentence that says what the application does when the
    /// variable is unset, if the file states one in its `effect` field.
    ///
    /// ```
    /// let spec = starmap::Spec::parse(
    ///     r#"
    /// [variables.SHELF_RECORDING_BUCKET]
    /// purpose = "The bucket that call recordings are written to."
    /// effect = "Calls are not recorded."
    /// type = "string"
    /// unset = "optional"
    /// show = "shown"
    /// source = "setting"
    /// "#,
    /// )?;
    /// let name: starmap::Name = "SHELF_RECORDING_BUCKET".parse()?;
    /// let variable = spec.variable(&name).ok_or("SHELF_RECORDING_BUCKET is declared")?;
    /// assert_eq!(variable.effect(), Some("Calls are not recorded."));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn effect(&self) -> Option<&str> {
        self.effect.as_deref()
    }

    /// Returns the sentence that states the check the application's own
    /// code makes of the value, if the file states one in its `check`
    /// field, such as `The port must be above 1024.`. starmap never runs
    /// it: a struct that derives [`Env`](crate::Env) runs the check its
    /// field names, and the sentence tells a reader that the check exists.
    #[must_use]
    pub fn check(&self) -> Option<&str> {
        self.check.as_deref()
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

    /// Returns the variable's `[variables.X.tool.<name>]` table called
    /// `name`, if it has one.
    #[must_use]
    pub fn tool(&self, name: &str) -> Option<&ToolTable> {
        find_tool(&self.tools, name)
    }

    /// Returns each of the variable's tool tables, in name order.
    pub fn tools(&self) -> impl Iterator<Item = &ToolTable> {
        self.tools.iter()
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
        /// How many random bytes the value has. For a variable with a
        /// bytes type it is the type's length, or its smallest length, and
        /// otherwise it is the `bytes` field, or 32 without one.
        bytes: ByteCount,
        /// The encoding the host writes the bytes in: the bytes type's
        /// encoding, or [`Encoding::Base64Url`] for a variable with the
        /// type string.
        encoding: Encoding,
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
    /// A string type's pattern is over [`Pattern::MAX_BYTES`](crate::Pattern::MAX_BYTES)
    /// or does not compile.
    BadPattern,
    /// An enumeration, a url type's scheme list, or the mode's production
    /// list has no values or repeats one.
    EmptyEnumeration,
    /// A secret or generated variable is shown.
    ShownSecret,
    /// A withheld variable has a type other than string, with or without a
    /// pattern, and bytes.
    WithheldType,
    /// A withheld variable has a default, which `starmap.toml` would hold in
    /// plain text.
    WithheldDefault,
    /// A bind does not fit the source, or a provided source has no
    /// provided bind.
    BindMismatch,
    /// `bytes` lies outside 16 to 1024, or appears on a variable that is
    /// not generated; a bytes type's length lies outside 1 to 1024; or a
    /// generated variable has a bytes type.
    BadBytes,
    /// A path template names a placeholder other than `{workspace}`,
    /// `{checkout}` and `{home}`, or has a brace outside one.
    UnknownPlaceholder,
    /// `[mode]` names a variable the file does not declare.
    ModeUndeclared,
    /// The mode variable is not a setting or inherited variable.
    ModeSource,
    /// A file has `spec`, which makes it a [`Pointer`], and declares
    /// variables, enumerations, groups or a mode too.
    PointerDeclares,
    /// A [`Pointer`] is read where a file that declares variables belongs,
    /// such as the file that another pointer names.
    PointerChain,
    /// A variable's type names an enumeration that the file does not
    /// declare in an `[enumerations.<name>]` table.
    EnumerationUndeclared,
    /// A group lists a variable that the file does not declare.
    GroupUndeclared,
    /// A group lists fewer than two variables or repeats one, or lists a
    /// variable that another group lists, that has a default or that is
    /// generated.
    BadGroup,
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
            Self::BadPattern => "starmap::spec::bad_pattern",
            Self::EmptyEnumeration => "starmap::spec::empty_enumeration",
            Self::ShownSecret => "starmap::spec::shown_secret",
            Self::WithheldType => "starmap::spec::withheld_type",
            Self::WithheldDefault => "starmap::spec::withheld_default",
            Self::BindMismatch => "starmap::spec::bind_mismatch",
            Self::BadBytes => "starmap::spec::bad_bytes",
            Self::UnknownPlaceholder => "starmap::spec::unknown_placeholder",
            Self::ModeUndeclared => "starmap::spec::mode_undeclared",
            Self::ModeSource => "starmap::spec::mode_source",
            Self::PointerDeclares => "starmap::spec::pointer_declares",
            Self::PointerChain => "starmap::spec::pointer_chain",
            Self::EnumerationUndeclared => "starmap::spec::enumeration_undeclared",
            Self::GroupUndeclared => "starmap::spec::group_undeclared",
            Self::BadGroup => "starmap::spec::bad_group",
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
    ("url", Type::Url { schemes: None }),
    ("socket-address", Type::SocketAddress),
    ("boolean", Type::Boolean),
    ("string", Type::String { pattern: None }),
];
/// The words a bytes type's `encoding` takes.
const ENCODING_WORDS: &[(&str, Encoding)] = &[
    ("hex", Encoding::Hex),
    ("base64", Encoding::Base64),
    ("base64url", Encoding::Base64Url),
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
