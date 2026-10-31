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
//! span of text it is about. The checks and the environment that
//! applies checked values to a child process arrive in later releases.
//!
//! The crate reads no environment variable itself. A caller passes each
//! value in.

mod bind;
mod form;
mod mode;
mod name;
mod spec;

pub use bind::{
    BytesError, OnePasswordRef, OnePasswordRefError, PathTemplate, PathTemplateError, Placeholder,
    ProvidedRef, ProviderKind, ProviderKindError, QualifierError, SecretBytes, SecretRef,
};
pub use form::{
    Choice, ChoiceError, Choices, ChoicesError, Form, FormError, IntegerRange, Port, PortError,
    RangeError, Value,
};
pub use mode::{Mode, ModeRule};
pub use name::{Name, NameError};
pub use spec::{
    Digest, Show, Source, Spec, SpecError, SpecProblem, SpecProblemKind, Unset, UnsetRule, Variable,
};
