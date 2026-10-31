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
//! The crate reads no environment variable itself. A caller passes each
//! value in.

mod bind;
mod check;
mod env;
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
