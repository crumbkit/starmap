//! starmap declares every environment variable an application reads. An
//! application keeps a `starmap.toml` beside its code, reads its environment
//! into a typed struct, and a test fails when the struct and the file
//! disagree.
//!
//! [`Spec::parse`] reads a `starmap.toml` into a [`Spec`]: each variable's
//! [`Name`], the [`Form`] its value takes with the [`Value`] that form
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
mod form;
mod mode;
mod name;
mod secret;
mod spec;

pub use bind::{
    Bindings, Bound, BytesError, OnePasswordRef, OnePasswordRefError, PathTemplate,
    PathTemplateError, Placeholder, ProvidedRef, ProviderKind, ProviderKindError, QualifierError,
    SecretBytes, SecretRef,
};
pub use check::{Finding, Problem, Refusals, Verdict, capture, check, mode};
pub use env::Environment;
pub use field::{Field, FieldError, FieldType, Presence, Shape};
pub use form::{
    Choice, ChoiceError, Choices, ChoicesError, Form, FormError, IntegerRange, Port, PortError,
    RangeError, Value,
};
pub use mode::{Mode, ModeRule};
pub use name::{Name, NameError};
pub use secret::Secret;
pub use spec::{
    Digest, Show, Source, Spec, SpecError, SpecProblem, SpecProblemKind, Unset, UnsetRule, Variable,
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
/// names, relative to the crate root. Each field names its variable with
/// `#[starmap("NAME")]`, and its type is one of `PathBuf`,
/// [`Port`], `url::Url`, `bool`, an integer type of at most 64 bits,
/// `String`, `OsString`, [`Secret<String>`] or a type that derives
/// [`Enumeration`], each inside an `Option` or not. Any other type fails to
/// compile.
#[cfg(feature = "derive")]
pub use starmap_derive::Env;

/// A struct that holds an application's environment, which
/// `#[derive(Env)]` implements. Each field reads one variable that the
/// embedded `starmap.toml` declares.
pub trait Env: Sized {
    /// The text of the embedded `starmap.toml`.
    const SPEC: &'static str;
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
    /// variable's form, its unset rule and its default, and the mode
    /// variable decides the mode. A variable that the spec does not
    /// declare is ignored.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError::Spec`] when the embedded spec does not parse,
    /// [`ReadError::Refused`] when a variable is missing or does not fit
    /// its form, and [`ReadError::Field`] when a field cannot hold a
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
#[diagnostic::on_unimplemented(
    message = "starmap cannot read a variable into `{Self}`",
    label = "this field type is not one that starmap reads",
    note = "a field of a struct that derives starmap::Env is PathBuf, starmap::Port, url::Url, bool, an integer type of at most 64 bits, String, OsString, starmap::Secret<String> or a type that derives starmap::Enumeration, and each may be inside an Option"
)]
pub trait Enumeration: Sized {
    /// The value of each variant, in the order the enum lists them.
    const VALUES: &'static [&'static str];

    /// Returns the variant whose value is `choice`, if there is one.
    fn from_choice(choice: &Choice) -> Option<Self>;
}

/// The reason [`Env::from_vars`] cannot read an environment. A spec or
/// field error is its `source()` (BASE-47), and a refusal's findings are
/// part of its text.
#[derive(Debug)]
#[non_exhaustive]
pub enum ReadError {
    /// The embedded `starmap.toml` does not parse.
    Spec(SpecError),
    /// A variable is missing or does not fit its form.
    Refused(Refusals),
    /// A field cannot hold its variable's checked value.
    Field(FieldError),
}

impl fmt::Display for ReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spec(_) => formatter.write_str("the embedded starmap.toml is invalid"),
            Self::Refused(refusals) => {
                formatter.write_str("the environment does not fit starmap.toml")?;
                for (index, finding) in refusals.iter().enumerate() {
                    formatter.write_str(if index == 0 { ": " } else { "; " })?;
                    let name = &finding.name;
                    match &finding.problem {
                        Problem::Missing { mode } => {
                            write!(
                                formatter,
                                "{name} is required in {mode} mode and has no value"
                            )
                        }
                        Problem::Invalid(error) => write!(formatter, "{name}: {error}"),
                        Problem::ShowMismatch { show } => {
                            write!(
                                formatter,
                                "{name} has a value of the wrong kind for a {show} variable"
                            )
                        }
                        Problem::ModeMismatch { decided } => {
                            write!(formatter, "{name} decides {decided} mode")
                        }
                    }?;
                }
                Ok(())
            }
            Self::Field(_) => formatter.write_str("a field of the struct cannot hold its value"),
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
