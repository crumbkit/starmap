//! The fields of a struct that derives [`Env`](crate::Env): the shape and
//! presence each field's type gives it, and the types a field may hold.

use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;

use url::Url;

use crate::{Enumeration, Environment, Name, Port, Secret, SecretBytes, Show, Value};

#[cfg(test)]
mod tests;

/// One field of a struct that derives [`Env`](crate::Env): the variable it
/// reads and what its type can hold. `starmap_test::check` compares each
/// field with the variable's declaration. The derive builds each field,
/// in [`Env::FIELDS`](crate::Env::FIELDS), and code outside starmap only
/// reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Field {
    /// The name of the variable the field reads.
    pub name: &'static str,
    /// The field's type as the struct writes it, without the `Option`
    /// around it, such as `url::Url` for `Option<url::Url>`.
    pub rust_type: &'static str,
    /// The kind of value the field's type holds.
    pub shape: Shape,
    /// Whether the field's type holds an unset variable.
    pub presence: Presence,
    /// Whether the field's type holds a withheld value.
    pub show: Show,
}

impl Field {
    /// Describes a field of type `T`, or `Option<T>` when `presence` is
    /// [`Presence::Maybe`], that reads the variable `name`. The derive
    /// calls it, and `rust_type` is `T` as the struct writes it. It is
    /// hidden because its bound is the derive's private `FieldType`.
    #[doc(hidden)]
    #[must_use]
    pub const fn new<T: FieldType>(
        name: &'static str,
        rust_type: &'static str,
        presence: Presence,
    ) -> Self {
        Self {
            name,
            rust_type,
            shape: T::SHAPE,
            presence,
            show: T::SHOW,
        }
    }
}

/// Reads a [`Field`]'s value from an [`Environment`]. The derive's
/// generated code calls its methods with method syntax, because rustc then
/// reports an unsupported field type once, at `Field::new`.
pub trait ReadField {
    /// Reads the field's value from `environment` as a `T`. It returns
    /// `Ok(None)` when the variable stays unset.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError`] when the field's name is not a variable name,
    /// or when `T` cannot hold the variable's checked value, such as a `u8`
    /// for the value 300.
    fn read<T: FieldType>(&self, environment: &Environment) -> Result<Option<T>, FieldError>;

    /// Reads the field's value from `environment` as a `T`, which the
    /// variable must have.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError`] when [`ReadField::read`] does, or when the
    /// variable stays unset.
    fn require<T: FieldType>(&self, environment: &Environment) -> Result<T, FieldError>;
}

impl ReadField for Field {
    fn read<T: FieldType>(&self, environment: &Environment) -> Result<Option<T>, FieldError> {
        let error = |problem| FieldError {
            name: self.name,
            problem,
        };
        let name: Name = self
            .name
            .parse()
            .map_err(|_| error(FieldProblem::Disagreement))?;
        if !environment.is_set(&name) {
            return Ok(None);
        }
        T::read(environment, &name).map(Some).map_err(error)
    }

    fn require<T: FieldType>(&self, environment: &Environment) -> Result<T, FieldError> {
        self.read(environment)?.ok_or(FieldError {
            name: self.name,
            problem: FieldProblem::Disagreement,
        })
    }
}

/// The kind of value a field's type holds. An integer type's bounds are
/// held as `i64`, the bounds of an integer type, so a type wider than `i64`
/// gives the bound of `i64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Shape {
    /// A path, held as a `PathBuf`.
    Path,
    /// A port, held as a [`Port`].
    Port,
    /// A URL, held as a [`Url`].
    Url,
    /// An IP socket address, held as a [`SocketAddr`].
    SocketAddress,
    /// A boolean, held as a `bool`.
    Boolean,
    /// A whole number from `min` to `max`, held as an integer type.
    Integer {
        /// The smallest number the type holds.
        min: i64,
        /// The largest number the type holds.
        max: i64,
    },
    /// One of these values, held as a type that derives
    /// [`Enumeration`](crate::Enumeration).
    Enumeration(&'static [&'static str]),
    /// Unicode text, held as a `String`, or as a [`Secret`] when
    /// it is withheld.
    String,
    /// The text of a string variable or the path of a path variable, held
    /// as an `OsString`. A path need not be Unicode.
    OsString,
    /// The decoded bytes of a bytes variable, held as a `Vec<u8>`, or as a
    /// [`SecretBytes`] when it is withheld.
    Bytes,
}

/// Whether a field's type holds an unset variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Presence {
    /// The field's type is not an `Option`, so the variable always has a
    /// value after defaults apply.
    Always,
    /// The field's type is an `Option`, so the variable may stay unset.
    Maybe,
}

/// A type that a field of a struct that derives [`Env`](crate::Env) holds,
/// inside an `Option` or not. starmap implements it for each type the
/// derive reads, and `#[derive(Enumeration)]` implements it for its enum.
#[diagnostic::on_unimplemented(
    message = "starmap cannot read a variable into `{Self}`",
    label = "this field type is not one that starmap reads",
    note = "a field of a struct that derives starmap::Env is PathBuf, starmap::Port, url::Url, std::net::SocketAddr, bool, an integer type of at most 64 bits, String, OsString, Vec<u8>, starmap::Secret, starmap::SecretBytes or a type that derives starmap::Enumeration, and each may be inside an Option"
)]
pub trait FieldType: Sized {
    /// The kind of value the type holds.
    const SHAPE: Shape;
    /// Whether the type holds a withheld value.
    const SHOW: Show;
    /// The reason that a field's own check of this type returns when it
    /// refuses a value: `String` for a shown type, and `&'static str` for a
    /// withheld one, so that the reason cannot hold the secret's text.
    type Reason: Into<String>;

    /// Reads the checked value of `name`, which is set, from
    /// `environment`.
    ///
    /// # Errors
    ///
    /// Returns the [`FieldProblem`] when the type cannot hold the value.
    fn read(environment: &Environment, name: &Name) -> Result<Self, FieldProblem>;
}

/// Implements [`FieldType`] for a shown type that reads one kind of
/// [`Value`], where `$read` returns `None` for a value the type cannot hold.
macro_rules! shown {
    ($type:ty, $shape:expr, |$value:ident| $read:expr) => {
        impl FieldType for $type {
            const SHAPE: Shape = $shape;
            const SHOW: Show = Show::Shown;
            type Reason = String;

            fn read(environment: &Environment, name: &Name) -> Result<Self, FieldProblem> {
                let read = |$value: &Value| -> Option<Self> { $read };
                environment
                    .value(name)
                    .and_then(read)
                    .ok_or(FieldProblem::Disagreement)
            }
        }
    };
}

shown!(PathBuf, Shape::Path, |value| match value {
    Value::Path(path) => Some(path.clone()),
    _ => None,
});
shown!(Port, Shape::Port, |value| match value {
    Value::Port(port) => Some(*port),
    _ => None,
});
shown!(Url, Shape::Url, |value| match value {
    Value::Url(url) => Some(url.clone()),
    _ => None,
});
shown!(SocketAddr, Shape::SocketAddress, |value| match value {
    Value::SocketAddress(address) => Some(*address),
    _ => None,
});
shown!(bool, Shape::Boolean, |value| match value {
    Value::Boolean(boolean) => Some(*boolean),
    _ => None,
});
shown!(Vec<u8>, Shape::Bytes, |value| match value {
    Value::Bytes { bytes, .. } => Some(bytes.clone()),
    _ => None,
});
shown!(OsString, Shape::OsString, |value| match value {
    Value::String(string) => Some(string.into()),
    Value::Path(path) => Some(path.clone().into_os_string()),
    _ => None,
});

/// Implements [`FieldType`] for integer types, whose bounds are clamped to
/// the bounds of `i64`.
macro_rules! integers {
    ($($type:ty),*) => {$(
        shown!(
            $type,
            Shape::Integer { min: clamp(<$type>::MIN as i128), max: clamp(<$type>::MAX as i128) },
            |value| match value {
                Value::Integer(number) => <$type>::try_from(*number).ok(),
                _ => None,
            }
        );
    )*};
}

integers!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

/// Returns `bound` clamped to the bounds of `i64`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "BASE-12: the value is clamped to the bounds of i64 first, and From is not const"
)]
const fn clamp(bound: i128) -> i64 {
    if bound > i64::MAX as i128 {
        i64::MAX
    } else if bound < i64::MIN as i128 {
        i64::MIN
    } else {
        bound as i64
    }
}

impl FieldType for String {
    const SHAPE: Shape = Shape::String;
    const SHOW: Show = Show::Shown;
    type Reason = String;

    fn read(environment: &Environment, name: &Name) -> Result<Self, FieldProblem> {
        match environment.value(name) {
            Some(Value::String(string)) => Ok(string.clone()),
            _ => Err(FieldProblem::Disagreement),
        }
    }
}

/// Reads the checked value of `name` from `environment` as the enum `E`.
/// The `FieldType` impl that `#[derive(Enumeration)]` writes calls it.
///
/// # Errors
///
/// Returns [`FieldProblem::Disagreement`] when the value is not one of
/// `E`'s values.
pub fn read_choice<E: Enumeration>(
    environment: &Environment,
    name: &Name,
) -> Result<E, FieldProblem> {
    match environment.value(name) {
        Some(Value::Choice(choice)) => E::from_choice(choice),
        _ => None,
    }
    .ok_or(FieldProblem::Disagreement)
}

impl FieldType for Secret {
    const SHAPE: Shape = Shape::String;
    const SHOW: Show = Show::Withheld;
    type Reason = &'static str;

    fn read(environment: &Environment, name: &Name) -> Result<Self, FieldProblem> {
        environment
            .secret(name)
            .map(Secret::duplicate)
            .ok_or(FieldProblem::Disagreement)
    }
}

impl FieldType for SecretBytes {
    const SHAPE: Shape = Shape::Bytes;
    const SHOW: Show = Show::Withheld;
    type Reason = &'static str;

    fn read(environment: &Environment, name: &Name) -> Result<Self, FieldProblem> {
        environment
            .secret_bytes(name)
            .map(SecretBytes::duplicate)
            .ok_or(FieldProblem::Disagreement)
    }
}

/// The application's own check of one field, which the derive writes for
/// a field attribute such as `#[starmap("SHELF_PORT", check = unprivileged)]`.
/// [`Env::from_vars`](crate::Env::from_vars) runs it inside the check of the
/// bindings, on each variable that holds a checked value.
#[derive(Debug, Clone, Copy)]
pub struct Rule {
    /// The name of the variable the field reads.
    pub(crate) name: &'static str,
    /// Reads the variable's checked value into the field's type and runs
    /// the application's check of it.
    pub(crate) run: fn(&Environment, &Name) -> Result<(), String>,
}

impl Rule {
    /// Returns the rule of the field that reads `name`, which `run` checks.
    #[must_use]
    pub const fn new(
        name: &'static str,
        run: fn(&Environment, &Name) -> Result<(), String>,
    ) -> Self {
        Self { name, run }
    }
}

/// Reads the checked value of `name` from `environment` as a `T` and
/// passes it to `check`, the application's check of the field. A variable
/// that stays unset is not checked. A value that `T` cannot hold is not
/// checked either, because [`Env::from_environment`](crate::Env::from_environment)
/// reports it.
///
/// # Errors
///
/// Returns the reason that `check` gives when it refuses the value.
pub fn run_check<T: FieldType>(
    environment: &Environment,
    name: &Name,
    check: fn(&T) -> Result<(), T::Reason>,
) -> Result<(), String> {
    if !environment.is_set(name) {
        return Ok(());
    }
    match T::read(environment, name) {
        Ok(value) => check(&value).map_err(Into::into),
        Err(_) => Ok(()),
    }
}

/// A field of a struct that derives [`Env`](crate::Env) cannot hold its
/// variable's checked value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldError {
    name: &'static str,
    problem: FieldProblem,
}

impl FieldError {
    /// Returns the name of the variable the field reads.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Returns why the field cannot hold the value.
    #[must_use]
    pub fn problem(&self) -> FieldProblem {
        self.problem
    }
}

/// Why a field cannot hold its variable's checked value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FieldProblem {
    /// The struct and its `starmap.toml` disagree about the variable, which
    /// `starmap_test::check` reports.
    Disagreement,
}

impl fmt::Display for FieldError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.name;
        match self.problem {
            FieldProblem::Disagreement => write!(
                formatter,
                "the field that reads {name} cannot hold its checked value, because the struct and starmap.toml disagree"
            ),
        }
    }
}

impl Error for FieldError {}
