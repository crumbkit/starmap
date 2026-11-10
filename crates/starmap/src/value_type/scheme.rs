//! The schemes a url type allows ([`Schemes`]), and one of them
//! ([`Scheme`]).

use std::error::Error;
use std::fmt;
use std::str::FromStr;

use super::{distinct, write_list};

#[cfg(test)]
mod tests;

/// A URL scheme in lower case, as RFC 3986 section 3.1 writes one: an ASCII
/// letter followed by letters, digits, `+`, `-` and `.`.
///
/// ```
/// let scheme: starmap::Scheme = "postgres".parse()?;
/// assert_eq!(scheme.as_str(), "postgres");
/// # Ok::<(), starmap::SchemeError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scheme(String);

impl Scheme {
    /// Returns the scheme as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Scheme {
    type Err = SchemeError;

    /// Accepts a lower case ASCII letter followed by lower case ASCII
    /// letters, digits, `+`, `-` and `.`. RFC 3986 lets a scheme hold upper
    /// case letters and says they mean the same as lower case ones; starmap
    /// refuses them so that a file writes each scheme one way.
    ///
    /// # Errors
    ///
    /// Returns [`SchemeError`] for empty text or text that breaks the rule.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut bytes = text.bytes();
        let starts = bytes.next().is_some_and(|first| first.is_ascii_lowercase());
        let rest = bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"+-.".contains(&byte)
        });
        if starts && rest {
            Ok(Self(text.to_owned()))
        } else {
            Err(SchemeError(()))
        }
    }
}

impl fmt::Display for Scheme {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The reason text is not a [`Scheme`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemeError(pub(super) ());

impl fmt::Display for SchemeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "a scheme is a lower case ascii letter followed by lower case letters, digits, +, - \
             and .",
        )
    }
}

impl Error for SchemeError {}

/// The schemes a url type allows: at least one [`Scheme`], none of them
/// repeated, in the order the spec lists them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Schemes(Vec<Scheme>);

impl Schemes {
    /// Builds the list from `schemes`, keeping their order.
    ///
    /// # Errors
    ///
    /// Returns [`SchemesError`] when there is no scheme or one repeats.
    pub fn new(schemes: impl IntoIterator<Item = Scheme>) -> Result<Self, SchemesError> {
        let kept = distinct(schemes).map_err(SchemesError::Repeated)?;
        if kept.is_empty() {
            return Err(SchemesError::Empty);
        }
        Ok(Self(kept))
    }

    /// Returns the schemes in order.
    pub fn iter(&self) -> impl Iterator<Item = &Scheme> {
        self.0.iter()
    }

    /// Returns whether the list holds the scheme `text`, compared exactly.
    pub(crate) fn contains(&self, text: &str) -> bool {
        self.0.iter().any(|scheme| scheme.as_str() == text)
    }
}

impl fmt::Display for Schemes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_list(formatter, &self.0)
    }
}

/// The reason a list of schemes is not [`Schemes`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SchemesError {
    /// The list is empty.
    Empty,
    /// The list holds this scheme more than once.
    Repeated(Scheme),
}

impl fmt::Display for SchemesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("a url's scheme list has no schemes"),
            Self::Repeated(scheme) => {
                write!(
                    formatter,
                    "a url's scheme list lists the scheme {scheme} twice"
                )
            }
        }
    }
}

impl Error for SchemesError {}
