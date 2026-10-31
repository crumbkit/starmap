//! starmap declares every environment variable an application reads. An
//! application keeps a `starmap.toml` beside its code, reads its environment
//! into a typed struct, and a test fails when the struct and the file
//! disagree.
//!
//! This release holds the types a declaration is made of: a variable's
//! [`Name`], the [`Form`] its value takes with the [`Value`] that form
//! reads, and the [`ModeRule`] that decides whether the application runs in
//! local or production [`Mode`]. The parser of `starmap.toml`, the checks
//! and the environment that applies checked values to a child process
//! arrive in later releases.
//!
//! The crate reads no environment variable itself. A caller passes each
//! value in.

mod form;
mod mode;
mod name;

pub use form::{
    Choice, ChoiceError, Choices, ChoicesError, Form, FormError, IntegerRange, Port, PortError,
    RangeError, Value,
};
pub use mode::{Mode, ModeRule};
pub use name::{Name, NameError};
