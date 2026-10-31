//! The fields of a struct that derives [`Env`](crate::Env): the shape and
//! presence each field's type gives it, and the types a field may hold.

use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

use url::Url;

use crate::{Enumeration, Environment, Name, Port, Secret, Show, Value};

#[cfg(test)]
mod tests;

/// One field of a struct that derives [`Env`](crate::Env): the variable it
/// reads and what its type can hold. `starmap_test::check` compares each
/// field with the variable's declaration.
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
    /// calls it, and `rust_type` is `T` as the struct writes it.
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

    /// Reads the field's value from `environment` as a `T`. It returns
    /// `Ok(None)` when the variable stays unset.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError`] when the field's name is not a variable name,
    /// or when `T` cannot hold the variable's checked value, such as a `u8`
    /// for the value 300.
    pub fn read<T: FieldType>(&self, environment: &Environment) -> Result<Option<T>, FieldError> {
        let error = FieldError { name: self.name };
        let name: Name = self.name.parse().map_err(|_| error)?;
        if !environment.is_set(&name) {
            return Ok(None);
        }
        T::read(environment, &name).map(Some).ok_or(error)
    }

    /// Reads the field's value from `environment` as a `T`, which the
    /// variable must have.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError`] when [`Field::read`] does, or when the
    /// variable stays unset.
    pub fn require<T: FieldType>(&self, environment: &Environment) -> Result<T, FieldError> {
        self.read(environment)?
            .ok_or(FieldError { name: self.name })
    }
}

/// The kind of value a field's type holds. An integer type's bounds are
/// held as `i64`, the bounds of an integer form, so a type wider than `i64`
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
    /// Unicode text, held as a `String`, or as a [`Secret<String>`] when
    /// it is withheld.
    String,
    /// Text that need not be Unicode, from a string or a path variable,
    /// held as an `OsString`.
    OsString,
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
/// derive reads, and for every type that derives
/// [`Enumeration`](crate::Enumeration).
#[diagnostic::on_unimplemented(
    message = "starmap cannot read a variable into `{Self}`",
    label = "this field type is not one that starmap reads",
    note = "a field of a struct that derives starmap::Env is PathBuf, starmap::Port, url::Url, bool, an integer type of at most 64 bits, String, OsString, starmap::Secret<String> or a type that derives starmap::Enumeration, and each may be inside an Option"
)]
pub trait FieldType: Sized {
    /// The kind of value the type holds.
    const SHAPE: Shape;
    /// Whether the type holds a withheld value.
    const SHOW: Show;

    /// Reads the checked value of `name` from `environment`. It returns
    /// `None` when the variable is unset or the type cannot hold its value.
    fn read(environment: &Environment, name: &Name) -> Option<Self>;
}

/// Implements [`FieldType`] for a shown type that reads one kind of
/// [`Value`].
macro_rules! shown {
    ($type:ty, $shape:expr, |$value:ident| $read:expr) => {
        impl FieldType for $type {
            const SHAPE: Shape = $shape;
            const SHOW: Show = Show::Shown;

            fn read(environment: &Environment, name: &Name) -> Option<Self> {
                let $value = environment.value(name)?;
                $read
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
shown!(bool, Shape::Boolean, |value| match value {
    Value::Boolean(boolean) => Some(*boolean),
    _ => None,
});
shown!(String, Shape::String, |value| match value {
    Value::String(string) => string.clone().into_string().ok(),
    _ => None,
});
shown!(OsString, Shape::OsString, |value| match value {
    Value::String(string) => Some(string.clone()),
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

impl<E: Enumeration> FieldType for E {
    const SHAPE: Shape = Shape::Enumeration(E::VALUES);
    const SHOW: Show = Show::Shown;

    fn read(environment: &Environment, name: &Name) -> Option<Self> {
        match environment.value(name)? {
            Value::Choice(choice) => E::from_choice(choice),
            _ => None,
        }
    }
}

impl FieldType for Secret<String> {
    const SHAPE: Shape = Shape::String;
    const SHOW: Show = Show::Withheld;

    fn read(environment: &Environment, name: &Name) -> Option<Self> {
        environment.secret(name).map(Secret::duplicate)
    }
}

/// A field of a struct that derives [`Env`](crate::Env) cannot hold its
/// variable's checked value, because the struct and its `starmap.toml`
/// disagree. `starmap_test::check` reports each disagreement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldError {
    name: &'static str,
}

impl FieldError {
    /// Returns the name of the variable the field reads.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }
}

impl fmt::Display for FieldError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "the field that reads {} cannot hold its checked value, because the struct and starmap.toml disagree",
            self.name
        )
    }
}

impl Error for FieldError {}
