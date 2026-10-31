//! The name of an environment variable.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

#[cfg(test)]
mod tests;

/// The name of an environment variable that a `starmap.toml` declares.
///
/// A name matches `[A-Z_][A-Z0-9_]*` and has at most [`Name::MAX_BYTES`]
/// bytes. [`FromStr`] is the only way to build one.
///
/// ```
/// let name: starmap::Name = "SHELF_PORT".parse()?;
/// assert_eq!(name.as_str(), "SHELF_PORT");
/// # Ok::<(), starmap::NameError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(String);

impl Name {
    /// The most bytes a name may have.
    pub const MAX_BYTES: usize = 128;

    /// Returns the name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Name {
    type Err = NameError;

    /// Accepts a string exactly when it matches `[A-Z_][A-Z0-9_]*` and has at
    /// most [`Name::MAX_BYTES`] bytes.
    ///
    /// # Errors
    ///
    /// Returns [`NameError`] when the string is empty, is too long, or holds
    /// a character the pattern does not allow.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.is_empty() {
            return Err(NameError::Empty);
        }
        if text.len() > Self::MAX_BYTES {
            return Err(NameError::TooLong { bytes: text.len() });
        }
        for (at, character) in text.char_indices() {
            let allowed = character == '_'
                || character.is_ascii_uppercase()
                || (at > 0 && character.is_ascii_digit());
            if !allowed {
                return Err(NameError::Character { character, at });
            }
        }
        Ok(Self(text.to_owned()))
    }
}

impl fmt::Display for Name {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The reason a string is not a [`Name`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NameError {
    /// The string is empty.
    Empty,
    /// The string has more than [`Name::MAX_BYTES`] bytes.
    TooLong {
        /// The number of bytes the string has.
        bytes: usize,
    },
    /// The string holds a character the pattern does not allow at that
    /// place, such as a lower case letter, or a digit at the start.
    Character {
        /// The first character that does not fit.
        character: char,
        /// The character's byte offset in the string.
        at: usize,
    },
}

impl fmt::Display for NameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("a variable name is empty"),
            Self::TooLong { bytes } => write!(
                formatter,
                "a variable name has {bytes} bytes, and the limit is {}",
                Name::MAX_BYTES
            ),
            Self::Character { character, at } => write!(
                formatter,
                "a variable name holds {character:?} at byte {at}, and a name holds only \
                 upper case letters, digits and underscores and does not start with a digit"
            ),
        }
    }
}

impl Error for NameError {}
