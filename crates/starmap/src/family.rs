//! The value of a family of variables that a struct field reads: each set
//! member's key with its checked value.

use crate::field::{FieldError, FieldProblem, FieldType};
use crate::{Environment, Name};

#[cfg(test)]
mod tests;

/// The members of a family of variables that are set, each read into a
/// `T`, which a field of a struct that derives [`Env`](crate::Env) holds,
/// as `#[starmap(family = "SHELF_DB_URL")] db: starmap::Family<url::Url>`.
///
/// Each member is held under its key, the part of its name after the
/// family's prefix and an underscore, so `SHELF_DB_URL_EU` is held under
/// `EU`. Every method lists the members in key order. A member that is
/// unset is not held. A required listed family that passes the check holds
/// every listed key.
///
/// Only the derive builds one, so a `Family` always holds checked values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Family<T> {
    /// Each set member's key with its value, in key order.
    members: Vec<(String, T)>,
}

impl<T> Family<T> {
    /// Returns the value of the member whose key is `key`, if it is set.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&T> {
        self.members
            .iter()
            .find(|(held, _)| held == key)
            .map(|(_, value)| value)
    }

    /// Returns each set member's key with its value, in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        self.members
            .iter()
            .map(|(key, value)| (key.as_str(), value))
    }

    /// Returns each set member's key, in key order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.members.iter().map(|(key, _)| key.as_str())
    }

    /// Returns how many members are set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Returns whether no member is set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

/// Reads each set member of the family whose prefix is `prefix` from
/// `environment` into a `T`. The derive's generated code calls it.
///
/// # Errors
///
/// Returns [`FieldError`] when `prefix` is not a variable name, or when
/// `T` cannot hold a member's checked value.
pub fn read_family<T: FieldType>(
    environment: &Environment,
    prefix: &'static str,
) -> Result<Family<T>, FieldError> {
    let error = |problem| FieldError::new(prefix, problem);
    let prefix_name: Name = prefix
        .parse()
        .map_err(|_| error(FieldProblem::Disagreement))?;
    let members = environment
        .members(&prefix_name)
        .map(|(key, name)| T::read(environment, name).map(|value| (key.to_owned(), value)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(error)?;
    Ok(Family { members })
}
