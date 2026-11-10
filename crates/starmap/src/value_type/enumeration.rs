//! The name of an enumeration that a `starmap.toml` declares in its
//! `[enumerations]` table.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

/// The name of an enumeration that a `starmap.toml` declares as
/// `[enumerations.<name>]` and a variable names with
/// `type = { enumeration = "<name>" }`.
///
/// A name starts with a lower case ASCII letter and holds only lower case
/// ASCII letters, digits and hyphens. [`FromStr`] is the only way to build
/// one.
///
/// ```
/// let name: starmap::EnumerationName = "log-level".parse()?;
/// assert_eq!(name.as_str(), "log-level");
/// # Ok::<(), starmap::EnumerationNameError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnumerationName(String);

impl EnumerationName {
    /// Returns the name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for EnumerationName {
    type Err = EnumerationNameError;

    /// Accepts text that starts with a lower case ASCII letter and holds
    /// only lower case ASCII letters, digits and hyphens.
    ///
    /// # Errors
    ///
    /// Returns [`EnumerationNameError`] for empty text, or text with a
    /// character that the rule does not allow at its place.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.is_empty() {
            return Err(EnumerationNameError::Empty);
        }
        for (at, character) in text.char_indices() {
            let allowed = character.is_ascii_lowercase()
                || (at > 0 && (character.is_ascii_digit() || character == '-'));
            if !allowed {
                return Err(EnumerationNameError::Character { character, at });
            }
        }
        Ok(Self(text.to_owned()))
    }
}

impl fmt::Display for EnumerationName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The reason text is not an [`EnumerationName`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EnumerationNameError {
    /// The text is empty.
    Empty,
    /// The text holds a character that the rule does not allow at that
    /// place, such as an upper case letter, or a digit at the start.
    Character {
        /// The first character that does not fit.
        character: char,
        /// The character's byte offset in the text.
        at: usize,
    },
}

impl fmt::Display for EnumerationNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("an enumeration name is empty"),
            Self::Character { character, at } => write!(
                formatter,
                "an enumeration name holds {character:?} at byte {at}, and a name starts with a \
                 lower case letter and holds only lower case letters, digits and hyphens"
            ),
        }
    }
}

impl Error for EnumerationNameError {}
