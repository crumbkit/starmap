//! starmap declares every environment variable an application reads. An
//! application keeps a `starmap.toml` beside its code, reads its environment
//! into a typed struct, and a test fails when the struct and the file
//! disagree.
//!
//! [`Spec::parse`] reads a `starmap.toml` into a [`Spec`]: each variable's
//! [`Name`], the [`Type`] of its value with the [`Value`] that type
//! reads, its [`UnsetRule`], its [`Show`] and its [`Source`] merged with the
//! bind that source allows, and the [`ModeRule`] that decides whether the
//! application runs in local or production [`Mode`]. A file that breaks a
//! rule returns a [`SpecError`] that lists every [`SpecProblem`] with the
//! span of text it is about.
//!
//! [`capture`] binds the variables a spec declares from an environment,
//! [`mode`] decides the mode, and [`check`] checks the [`Bindings`] in that
//! mode. Its [`Verdict`] is an [`Environment`] of checked values, which
//! [`Environment::apply`] sets on a child process, or [`Refusals`] that
//! name each missing or invalid variable. A withheld value is held as a
//! [`Secret`], whose `Debug` prints only dots.
//!
//! An application reads its environment into a struct that derives
//! [`Env`], whose fields each read one variable, and an enum that derives
//! [`Enumeration`] reads an enumeration variable. The derives come with the
//! `derive` feature. [`Env::load`] reads the process environment, and
//! [`Env::from_vars`] reads a fixed set of variables, as a test does.
//!
//! Each [`SpecProblemKind`] has a failure code of the form
//! `starmap::<area>::<name>`, such as `starmap::spec::bad_default`, which
//! a host prints as it is.
//!
//! The `url` crate is part of starmap's public API, because [`Value::Url`]
//! holds a `url::Url` and [`TypeError::NotUrl`] holds its parse error.
//! starmap re-exports it as [`url`], so a host names the same version.
//!
//! The crate reads no environment variable itself. A caller passes each
//! value in, and the `load` that the derive generates in the application
//! crate reads the process environment.

use std::error::Error;
use std::ffi::OsString;
use std::fmt;

mod bind;
mod check;
mod env;
mod field;
mod mode;
mod name;
mod secret;
mod spec;
mod value_type;

pub use bind::{
    Bindings, Bound, BytesError, Cache, NoPlace, OnePasswordRef, OnePasswordRefError, PathTemplate,
    PathTemplateError, Placeholder, ProvidedRef, ProviderKind, ProviderKindError, QualifierError,
    SecretBind, SecretBytes, SecretRef,
};
pub use check::{Finding, Problem, Refusals, Verdict, capture, check, mode};
pub use env::Environment;
pub use field::{Field, FieldError, FieldProblem, Presence, Shape};
pub use mode::{Mode, ModeRule};
pub use name::{Name, NameError};
pub use secret::Secret;
pub use spec::{
    Digest, Pointer, Show, Source, Spec, SpecError, SpecProblem, SpecProblemKind, ToolTable, Unset,
    UnsetRule, Variable,
};
pub use url;
pub use value_type::{
    Choice, ChoiceError, Choices, ChoicesError, EnumerationName, EnumerationNameError,
    IntegerRange, Pattern, PatternError, Port, PortError, RangeError, Scheme, SchemeError, Schemes,
    SchemesError, Type, TypeError, Value,
};

/// Derives [`Enumeration`] for an enum of unit variants. A variant reads as
/// its name in kebab case, such as `dry-run` for `DryRun`, and
/// `#[starmap("prod")]` on a variant gives it another value.
#[cfg(feature = "derive")]
pub use starmap_derive::Enumeration;
/// Derives [`Env`] for a struct whose fields each read one variable.
///
/// The derive embeds the `starmap.toml` in the root of the crate, or the
/// file that `#[starmap(spec = "config/starmap.toml")]` on the struct
/// names, relative to the crate root. When that file is a [`Pointer`], the
/// derive embeds the file that the pointer names, and a pointer that also
/// declares variables, or names another pointer, fails to compile. Each field names its variable with
/// `#[starmap("NAME")]`, and its type is one of `PathBuf`,
/// [`Port`], `url::Url`, `std::net::SocketAddr`, `bool`, an integer type
/// of at most 64 bits, `String`, `OsString`, [`Secret`] or a type
/// that derives [`Enumeration`], each inside an `Option` or not. Any
/// other type fails to compile.
#[cfg(feature = "derive")]
pub use starmap_derive::Env;

/// The items that the derives' generated code names. They are not part of
/// starmap's public API, and any release may change them (LIB-7).
#[doc(hidden)]
pub mod __private {
    pub use crate::field::{FieldType, ReadField, read_choice};
}

/// A struct that holds an application's environment, which
/// `#[derive(Env)]` implements. Each field reads one variable that the
/// embedded `starmap.toml` declares.
pub trait Env: Sized {
    /// The text of the embedded `starmap.toml`.
    const SPEC: &'static str;
    /// The path of the file whose text [`Env::SPEC`] holds, relative to the
    /// crate root, such as `starmap.toml`. When the struct's file is a
    /// [`Pointer`], it is the file that the pointer names.
    const SPEC_FILE: &'static str;
    /// The struct's fields, in the order the struct lists them.
    const FIELDS: &'static [Field];

    /// Reads the process environment into the struct, through
    /// [`Env::from_vars`]. The derive generates it in the application
    /// crate, which owns the read of its environment (BASE-6, LIB-1). It
    /// never reads a `.env` file.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError`] as [`Env::from_vars`] does.
    fn load() -> Result<Self, ReadError>;

    /// Reads `vars` into the struct. The embedded spec gives each
    /// variable's type, its unset rule and its default, and the mode
    /// variable decides the mode. A variable that the spec does not
    /// declare is ignored. An empty value, such as the value of `FOO=`,
    /// counts as unset, so an `Option` field reads it as `None`, a field
    /// with a default reads the default, and a required field refuses the
    /// read.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError::Spec`] when the embedded spec does not parse,
    /// [`ReadError::Refused`] when a variable is missing or does not fit
    /// its type, and [`ReadError::Field`] when a field cannot hold a
    /// checked value.
    fn from_vars(vars: impl IntoIterator<Item = (OsString, OsString)>) -> Result<Self, ReadError> {
        let spec = Spec::parse(Self::SPEC).map_err(ReadError::Spec)?;
        let bindings = capture(&spec, vars);
        match check(&spec, &bindings, mode(&spec, &bindings)) {
            Verdict::Ready(environment) => {
                Self::from_environment(&environment).map_err(ReadError::Field)
            }
            Verdict::Refused(refusals) => Err(ReadError::Refused(refusals)),
        }
    }

    /// Builds the struct from the checked values of `environment`.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError`] when a field cannot hold its variable's
    /// checked value, because the struct and the spec disagree.
    fn from_environment(environment: &Environment) -> Result<Self, FieldError>;
}

/// An enum whose values an enumeration variable takes, which
/// `#[derive(Enumeration)]` implements.
pub trait Enumeration: Sized {
    /// The value of each variant, in the order the enum lists them.
    const VALUES: &'static [&'static str];

    /// Returns the variant whose value is `choice`, if there is one.
    fn from_choice(choice: &Choice) -> Option<Self>;

    /// Returns the variant's value, which is one of [`Enumeration::VALUES`].
    fn value(&self) -> &'static str;
}

/// The reason [`Env::from_vars`] cannot read an environment. A spec or
/// field error is its `source()` (BASE-47), and a refusal's findings are
/// its text, as [`Refusals`] writes them, so a program can print it after
/// its own words, such as `shelf cannot start: SHELF_PORT is required and
/// has no value`. A field error's text names the variable.
#[derive(Debug)]
#[non_exhaustive]
pub enum ReadError {
    /// The embedded `starmap.toml` does not parse.
    Spec(SpecError),
    /// A variable is missing or does not fit its type.
    Refused(Refusals),
    /// A field cannot hold its variable's checked value.
    Field(FieldError),
}

impl fmt::Display for ReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spec(_) => formatter.write_str("the embedded starmap.toml is invalid"),
            Self::Refused(refusals) => refusals.fmt(formatter),
            Self::Field(error) => write!(
                formatter,
                "the struct cannot read {} into its field",
                error.name()
            ),
        }
    }
}

impl Error for ReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Spec(error) => Some(error),
            Self::Refused(_) => None,
            Self::Field(error) => Some(error),
        }
    }
}
